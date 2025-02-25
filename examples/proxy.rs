use clap::Parser;
use get_if_addrs::get_if_addrs;
use rsip::prelude::{HeadersExt, ToTypedHeader};
use rsip::Request;
use rsipstack::transaction::key::{TransactionKey, TransactionRole};
use rsipstack::transaction::transaction::Transaction;
use rsipstack::transaction::TransactionReceiver;
use rsipstack::transport::connection::SipAddr;
use rsipstack::transport::udp::UdpConnection;
use rsipstack::transport::SipConnection;
use rsipstack::{transport::TransportLayer, EndpointBuilder};
use rsipstack::{Error, Result};
use serde::de;
use std::collections::HashMap;
use std::env;
use std::sync::{Arc, Mutex};
use tokio::select;
use tokio_util::sync::CancellationToken;
use tracing::info;
/// A SIP client example that sends a REGISTER request to a SIP server.
#[derive(Parser, Debug)]
#[command(author, version, about, long_about = None)]
struct Args {
    /// SIP port
    #[arg(long, default_value = "25060")]
    port: u16,

    /// External IP address
    #[arg(long)]
    external_ip: Option<String>,
}
#[derive(Clone)]
struct User {
    from: String,
    destination: SipAddr,
    alive_at: std::time::Instant,
}

struct Session {
    caller: String,
    calee: String,
}

struct AppState {
    users: Mutex<HashMap<String, User>>,
    sessions: Mutex<HashMap<String, Session>>,
}

#[tokio::main]
async fn main() -> Result<()> {
    tracing_subscriber::fmt()
        .with_max_level(tracing::Level::DEBUG)
        .with_file(true)
        .with_line_number(true)
        .try_init()
        .ok();
    if let Err(e) = dotenv::dotenv() {
        info!("Failed to load .env file: {}", e);
    }
    let args = Args::parse();

    let token = CancellationToken::new();
    let transport_layer = TransportLayer::new(token.clone());

    let external_ip = args
        .external_ip
        .unwrap_or(env::var("EXTERNAL_IP").unwrap_or_default());

    let external = if external_ip.is_empty() {
        None
    } else {
        Some(format!("{}:{}", external_ip, args.port).parse()?)
    };

    let addr = get_if_addrs()?
        .iter()
        .find(|i| !i.is_loopback())
        .map(|i| match i.addr {
            get_if_addrs::IfAddr::V4(ref addr) => Ok(std::net::IpAddr::V4(addr.ip)),
            _ => Err(Error::Error("No IPv4 address found".to_string())),
        })
        .unwrap_or(Err(Error::Error("No interface found".to_string())))?;

    let connection = UdpConnection::create_connection(
        format!("{}:{}", addr, args.port).parse()?,
        external.clone(),
    )
    .await?;

    transport_layer.add_transport(connection.into());

    let endpoint = EndpointBuilder::new()
        .cancel_token(token.clone())
        .transport_layer(transport_layer)
        .build();

    let incoming = endpoint.incoming_transactions();

    select! {
        _ = endpoint.serve() => {
            info!("user agent finished");
        }
        r = process_incoming_request(incoming) => {
            info!("serve loop finished {:?}", r);
        }
    }
    Ok(())
}

async fn process_incoming_request(mut incoming: TransactionReceiver) -> Result<()> {
    let state = Arc::new(AppState {
        users: Mutex::new(HashMap::new()),
        sessions: Mutex::new(HashMap::new()),
    });
    while let Some(mut tx) = incoming.recv().await {
        info!("Received transaction: {:?}", tx.key);
        let state = state.clone();
        match tx.original.method {
            rsip::Method::Invite | rsip::Method::Ack => {
                tokio::spawn(async move {
                    handle_invite(state, tx).await?;
                    Ok::<_, Error>(())
                });
            }
            rsip::Method::Bye => {
                tokio::spawn(async move {
                    handle_bye(state, tx).await?;
                    Ok::<_, Error>(())
                });
            }
            rsip::Method::Register => {
                handle_register(state, tx).await?;
            }
            _ => {
                info!("Received request: {:?}", tx.original.method);
                tx.reply(rsip::StatusCode::NotAcceptable).await?;
            }
        }
    }
    Ok::<_, Error>(())
}

impl TryFrom<&rsip::Request> for User {
    type Error = Error;
    fn try_from(req: &rsip::Request) -> Result<Self> {
        let contact = req.contact_header()?.clone();
        let via = req.via_header()?.typed()?;

        let mut destination = SipAddr {
            r#type: via.uri.transport().cloned(),
            addr: contact.uri()?.host_with_port,
        };

        via.params.iter().for_each(|param| match param {
            rsip::Param::Transport(t) => {
                destination.r#type = Some(t.clone());
            }
            rsip::Param::Received(r) => match r.value().to_string().as_str().try_into() {
                Ok(addr) => destination.addr.host = addr,
                Err(_) => {}
            },
            rsip::Param::Other(o, Some(v)) => {
                if o.value().eq_ignore_ascii_case("rport") {
                    match v.value().to_string().try_into() {
                        Ok(port) => destination.addr.port = Some(port),
                        Err(_) => {}
                    }
                }
            }
            _ => {}
        });

        Ok(User {
            from: req.from_header()?.uri()?.to_string(),
            destination,
            alive_at: std::time::Instant::now(),
        })
    }
}

async fn handle_register(state: Arc<AppState>, mut tx: Transaction) -> Result<()> {
    let user = match User::try_from(&tx.original) {
        Ok(u) => u,
        Err(_) => {
            return tx.reply(rsip::StatusCode::BadRequest).await;
        }
    };
    state.users.lock().unwrap().insert(user.from.clone(), user);
    let headers = vec![rsip::Header::Expires(60.into())];
    tx.reply_with(rsip::StatusCode::OK, headers, None).await
}

async fn handle_invite(state: Arc<AppState>, mut tx: Transaction) -> Result<()> {
    let caller = tx.original.from_header()?.uri()?.to_string();
    let callee = tx.original.to_header()?.uri()?.to_string();
    let target = state.users.lock().unwrap().get(&callee).cloned();
    let target = match target {
        Some(u) => u,
        None => {
            return tx.reply(rsip::StatusCode::NotFound).await;
        }
    };

    let mut inv_req = tx.original.clone();
    let via = tx.endpoint_inner.get_via(None)?;
    let mut headers = vec![rsip::Header::Via(via.into())];
    headers.extend(inv_req.headers.clone());
    inv_req.headers = headers.into();

    let key = TransactionKey::from_request(&inv_req, TransactionRole::Client)
        .expect("client_transaction");

    info!("Forwarding INVITE to: {} -> {}", caller, target.destination);

    let mut inv_tx = Transaction::new_client(key, inv_req, tx.endpoint_inner.clone(), None);
    inv_tx.destination = Some(target.destination);

    // add Via
    inv_tx.send().await?;

    while let Some(msg) = inv_tx.receive().await {
        match msg {
            rsip::message::SipMessage::Request(req) => {
                if req.method == rsip::Method::Ack {
                    break;
                }
            }
            rsip::message::SipMessage::Response(mut resp) => {
                // pop first Via
                let mut first_via = true;
                resp.headers.retain(|h| match h {
                    rsip::Header::Via(_) => {
                        if first_via {
                            first_via = false;
                            false
                        } else {
                            true
                        }
                    }
                    _ => true,
                });
                info!("Forwarding response: {:?}", resp);
                tx.respond(resp).await?;
            }
        }
    }
    Ok(())
}

async fn handle_bye(state: Arc<AppState>, mut tx: Transaction) -> Result<()> {
    Ok(())
}
