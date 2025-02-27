use clap::Parser;
use get_if_addrs::get_if_addrs;
use rsip::headers::UntypedHeader;
use rsip::prelude::{HeadersExt, ToTypedHeader};
use rsipstack::rsip_ext::{extract_uri_from_contact, RsipHeadersExt};
use rsipstack::transaction::key::{TransactionKey, TransactionRole};
use rsipstack::transaction::transaction::Transaction;
use rsipstack::transaction::TransactionReceiver;
use rsipstack::transport::connection::SipAddr;
use rsipstack::transport::udp::UdpConnection;
use rsipstack::{header_pop, Error, Result};
use rsipstack::{transport::TransportLayer, EndpointBuilder};
use std::collections::HashMap;
use std::env;
use std::sync::{Arc, Mutex};
use tokio::select;
use tokio_util::sync::CancellationToken;
use tracing::{info, instrument};

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
    username: String,
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
            rsip::Method::Invite => {
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
        let contact = extract_uri_from_contact(req.contact_header()?.value())?;
        let via = req.via_header()?.typed()?;

        let mut destination = SipAddr {
            r#type: via.uri.transport().cloned(),
            addr: contact.host_with_port,
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
        let username = req
            .from_header()?
            .uri()?
            .user()
            .unwrap_or_default()
            .to_string();
        Ok(User {
            username,
            from: req.from_header()?.uri()?.to_string(),
            destination,
            alive_at: std::time::Instant::now(),
        })
    }
}

#[instrument(skip(state, tx), fields(reg = %tx.original))]
async fn handle_register(state: Arc<AppState>, mut tx: Transaction) -> Result<()> {
    let user = match User::try_from(&tx.original) {
        Ok(u) => u,
        Err(e) => {
            info!("failed to parse contact: {:?}", e);
            return tx.reply(rsip::StatusCode::BadRequest).await;
        }
    };
    let contact = rsip::typed::Contact {
        display_name: None,
        uri: rsip::Uri {
            scheme: Some(rsip::Scheme::Sip),
            auth: Some(rsip::Auth {
                user: user.username.clone(),
                password: None,
            }),
            host_with_port: user.destination.addr.clone(),
            ..Default::default()
        },
        params: vec![],
    };
    state.users.lock().unwrap().insert(user.from.clone(), user);
    let headers = vec![contact.into(), rsip::Header::Expires(60.into())];
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
    inv_req.headers.push_front(via.into());

    let key = TransactionKey::from_request(&inv_req, TransactionRole::Client)
        .expect("client_transaction");

    info!("Forwarding INVITE to: {} -> {}", caller, target.destination);

    let mut inv_tx = Transaction::new_client(key, inv_req, tx.endpoint_inner.clone(), None);
    inv_tx.destination = Some(target.destination);

    // add Via
    inv_tx.send().await?;

    loop {
        if inv_tx.is_terminated() {
            break;
        }
        select! {
            msg = inv_tx.receive() => {
                info!("UAC Received message: {:?}", msg);
                if let Some(msg) = msg {
                    match msg {
                        rsip::message::SipMessage::Response(mut resp) => {
                            // pop first Via
                            header_pop!(resp.headers, rsip::Header::Via);
                            info!("UAC Forwarding response: {:?}", resp);
                            tx.respond(resp).await?;
                        }
                        _ => {}
                    }
                }
            }
            msg = tx.receive() => {
                info!("UAS Received message: {:?}", msg);
                if let Some(msg) = msg {
                    match msg {
                        rsip::message::SipMessage::Request(req) => match req.method {
                            rsip::Method::Ack => {
                                inv_tx.send_ack(req).await?;
                            }
                            rsip::Method::Cancel => {
                                inv_tx.send_cancel(req).await?;
                            }
                            _ => {}
                        },
                        rsip::message::SipMessage::Response(resp) => {}
                    }
                }
            }
        }
    }
    Ok(())
}

async fn handle_bye(state: Arc<AppState>, mut tx: Transaction) -> Result<()> {
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
    inv_req.headers.push_front(via.into());

    let key = TransactionKey::from_request(&inv_req, TransactionRole::Client)
        .expect("client_transaction");

    info!("Forwarding INVITE to: {} -> {}", caller, target.destination);

    let mut bye_tx = Transaction::new_client(key, inv_req, tx.endpoint_inner.clone(), None);
    bye_tx.destination = Some(target.destination);

    // add Via
    bye_tx.send().await?;

    while let Some(msg) = bye_tx.receive().await {
        info!("UAC/BYE Received message: {:?}", msg);
        match msg {
            rsip::message::SipMessage::Response(mut resp) => {
                // pop first Via
                header_pop!(resp.headers, rsip::Header::Via);
                info!("UAC/BYE Forwarding response: {:?}", resp);
                tx.respond(resp).await?;
            }
            _ => {}
        }
    }

    Ok(())
}
