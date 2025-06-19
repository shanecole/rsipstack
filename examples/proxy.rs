use axum::response::Html;
use clap::Parser;
use futures::StreamExt;
use get_if_addrs::get_if_addrs;
use rsip::headers::UntypedHeader;
use rsip::prelude::{HeadersExt, ToTypedHeader};
use rsipstack::dialog::DialogId;
use rsipstack::rsip_ext::{extract_uri_from_contact, RsipHeadersExt};
use rsipstack::transaction::endpoint::EndpointInnerRef;
use rsipstack::transaction::key::{TransactionKey, TransactionRole};
use rsipstack::transaction::transaction::Transaction;
use rsipstack::transaction::TransactionReceiver;
use rsipstack::transport::udp::UdpConnection;
use rsipstack::transport::SipAddr;
use rsipstack::{header_pop, Error, Result};
use rsipstack::{
    transport::{transport_layer::TransportConfig, TransportLayer},
    EndpointBuilder,
};
use std::collections::{HashMap, HashSet};
use std::sync::{Arc, Mutex};
use std::{env, vec};
use tokio::select;
use tokio_util::sync::CancellationToken;
use tracing::{error, info};

use axum::extract::ws::WebSocket;
use axum::{
    extract::{ws::WebSocketUpgrade, State},
    response::IntoResponse,
    routing::get,
    Router,
};
use std::net::SocketAddr;
use tower::ServiceBuilder;
use tower_http::cors::CorsLayer;

/// A SIP proxy server that supports UDP, TCP and WebSocket clients
#[derive(Parser, Debug)]
#[command(author, version, about, long_about = None)]
struct Args {
    /// SIP address
    #[arg(long)]
    addr: Option<String>,
    /// SIP port
    #[arg(long, default_value = "25060")]
    port: u16,

    /// External IP address
    #[arg(long)]
    external_ip: Option<String>,

    /// TCP port
    #[arg(long)]
    tcp_port: Option<u16>,

    /// WebSocket port
    #[arg(long)]
    ws_port: Option<u16>,

    /// HTTP server port for web interface
    #[arg(long)]
    http_port: Option<u16>,
}

#[derive(Clone)]
struct User {
    username: String,
    destination: SipAddr,
}

struct AppState {
    users: Mutex<HashMap<String, User>>,
    sessions: Mutex<HashSet<DialogId>>,
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
    let transport_config = TransportConfig {
        tls: None,
        enable_ws: true,
        enable_wss: true,
    };
    let transport_layer = TransportLayer::with_config(token.clone(), transport_config);

    let external_ip = args
        .external_ip
        .unwrap_or(env::var("EXTERNAL_IP").unwrap_or_default());

    let external = if external_ip.is_empty() {
        None
    } else {
        Some(format!("{}:{}", external_ip, args.port).parse()?)
    };
    let addr = match args.addr {
        Some(addr) => addr.parse::<std::net::IpAddr>()?,
        None => get_if_addrs()?
            .iter()
            .find(|i| !i.is_loopback())
            .map(|i| match i.addr {
                get_if_addrs::IfAddr::V4(ref addr) => Ok(std::net::IpAddr::V4(addr.ip)),
                _ => Err(Error::Error("No IPv4 address found".to_string())),
            })
            .unwrap_or(Err(Error::Error("No interface found".to_string())))?,
    };

    // Create a transport sender for adding listeners
    let (transport_tx, _transport_rx) = tokio::sync::mpsc::unbounded_channel();
    let connection = UdpConnection::create_connection(
        format!("{}:{}", addr, args.port).parse()?,
        external.clone(),
    )
    .await?;
    transport_layer.add_transport(connection.into());
    info!("Added UDP transport on {}:{}", addr, args.port);

    if let Some(tcp_port) = args.tcp_port {
        let tcp_addr = transport_layer
            .add_tcp_listener(
                format!("{}:{}", addr, tcp_port).parse()?,
                transport_tx.clone(),
            )
            .await?;
        info!("Added TCP transport on {}", tcp_addr);
    }

    let mut ws_port = args.ws_port;
    // Start HTTP server if http_port is specified
    let http_server_handle = if let Some(http_port) = args.http_port {
        let app_state = Arc::new(AppState {
            users: Mutex::new(HashMap::new()),
            sessions: Mutex::new(HashSet::new()),
        });

        let app = create_http_app(app_state.clone());
        let http_addr = format!("0.0.0.0:{}", http_port).parse::<SocketAddr>()?;
        ws_port = Some(http_port + 1);

        info!(
            "Starting HTTP server on {}, websocket port: {:?}",
            http_addr, ws_port
        );
        let listener = tokio::net::TcpListener::bind(http_addr).await?;
        Some(tokio::spawn(async move {
            axum::serve(listener, app).await.unwrap();
        }))
    } else {
        None
    };

    if let Some(ws_port) = ws_port {
        let ws_addr = transport_layer
            .add_ws_listener(
                format!("{}:{}", addr, ws_port).parse()?,
                transport_tx.clone(),
                false,
            )
            .await?;
        info!("Added WebSocket transport on {}", ws_addr);
    }

    let endpoint = EndpointBuilder::new()
        .with_cancel_token(token.clone())
        .with_transport_layer(transport_layer)
        .build();

    let incoming = endpoint.incoming_transactions();
    select! {
        _ = endpoint.serve() => {
            info!("proxy endpoint finished");
        }
        r = process_incoming_request(incoming) => {
            info!("serve loop finished {:?}", r);
        }
        _ = async {
            if let Some(handle) = http_server_handle {
                handle.await.unwrap();
            } else {
                std::future::pending().await
            }
        } => {
            info!("HTTP server finished");
        }
    }
    Ok(())
}

async fn process_incoming_request(mut incoming: TransactionReceiver) -> Result<()> {
    let state = Arc::new(AppState {
        users: Mutex::new(HashMap::new()),
        sessions: Mutex::new(HashSet::new()),
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
            rsip::Method::Ack => {
                info!("received out of transaction ack: {:?}", tx.original.method);
                let dialog_id = DialogId::try_from(&tx.original)?;
                if !state.sessions.lock().unwrap().contains(&dialog_id) {
                    tx.reply(rsip::StatusCode::NotAcceptable).await?;
                    continue;
                }
                // forward ACK
                let callee = tx
                    .original
                    .to_header()?
                    .uri()?
                    .auth
                    .map(|a| a.user)
                    .unwrap_or_default();
                let target = state.users.lock().unwrap().get(&callee).cloned();
                let target = match target {
                    Some(u) => u,
                    None => {
                        info!("ack user not found: {}", callee);
                        tx.reply(rsip::StatusCode::NotAcceptable).await?;
                        continue;
                    }
                };
                // send to target
                let mut ack_req = tx.original.clone();
                let via = tx.endpoint_inner.get_via(None, None)?;
                ack_req.headers.push_front(via.into());
                let key = TransactionKey::from_request(&ack_req, TransactionRole::Client)
                    .expect("client_transaction");
                let mut ack_tx =
                    Transaction::new_client(key, ack_req, tx.endpoint_inner.clone(), None);
                ack_tx.destination = Some(target.destination);
                ack_tx.send().await?;
            }
            _ => {
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
            rsip::Param::Received(r) => match r.value().try_into() {
                Ok(addr) => destination.addr.host = addr,
                Err(_) => {}
            },
            rsip::Param::Other(o, Some(v)) => {
                if o.value().eq_ignore_ascii_case("rport") {
                    match v.value().try_into() {
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
            destination,
        })
    }
}

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
    match tx.original.expires_header() {
        Some(expires) => match expires.value().parse::<u32>() {
            Ok(v) => {
                if v == 0 {
                    // remove user
                    info!("unregistered user: {} -> {}", user.username, contact);
                    state.users.lock().unwrap().remove(&user.username);
                    return tx.reply(rsip::StatusCode::OK).await;
                }
            }
            Err(_) => {}
        },
        None => {}
    }

    info!("Registered user: {} -> {}", user.username, contact);
    state
        .users
        .lock()
        .unwrap()
        .insert(user.username.clone(), user);

    let headers = vec![contact.into(), rsip::Header::Expires(60.into())];
    tx.reply_with(rsip::StatusCode::OK, headers, None).await
}

async fn handle_invite(state: Arc<AppState>, mut tx: Transaction) -> Result<()> {
    let caller = tx.original.from_header()?.uri()?.to_string();
    let callee = tx
        .original
        .to_header()?
        .uri()?
        .auth
        .map(|a| a.user)
        .unwrap_or_default();
    let target = state.users.lock().unwrap().get(&callee).cloned();

    let record_route = tx.endpoint_inner.get_record_route()?;

    let target = match target {
        Some(u) => u,
        None => {
            info!("user not found: {}", callee);
            tx.reply_with(rsip::StatusCode::NotFound, vec![record_route.into()], None)
                .await
                .ok();
            // wait for ACK
            while let Some(msg) = tx.receive().await {
                match msg {
                    rsip::message::SipMessage::Request(req) => match req.method {
                        rsip::Method::Ack => {
                            info!("received no-2xx ACK");
                            break;
                        }
                        _ => {}
                    },
                    _ => {}
                }
            }
            return Ok(());
        }
    };

    let mut inv_req = tx.original.clone();
    let via = tx.endpoint_inner.get_via(None, None)?;
    inv_req.headers.push_front(via.into());
    inv_req.headers.push_front(record_route.clone().into());
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
                info!("UAC Received message: {}", msg.as_ref().map(|m| m.to_string()).unwrap_or_default());
                if let Some(msg) = msg {
                    match msg {
                        rsip::message::SipMessage::Response(mut resp) => {
                            // pop first Via
                            header_pop!(resp.headers, rsip::Header::Via);
                            resp.headers.push_front(record_route.clone().into());
                            if resp.status_code.kind() == rsip::StatusCodeKind::Successful {
                                let dialog_id = DialogId::try_from(&resp)?;
                                info!("add session: {}", dialog_id);
                                state.sessions.lock().unwrap().insert(dialog_id);
                            }
                            tx.respond(resp).await?;
                        }
                        _ => {}
                    }
                }
            }
            msg = tx.receive() => {
                info!("UAS Received message: {}", msg.as_ref().map(|m| m.to_string()).unwrap_or_default());
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
                        rsip::message::SipMessage::Response(_) => {}
                    }
                }
            }
        }
    }
    Ok(())
}

async fn handle_bye(state: Arc<AppState>, mut tx: Transaction) -> Result<()> {
    let caller = tx.original.from_header()?.uri()?.to_string();
    let callee = tx
        .original
        .to_header()?
        .uri()?
        .auth
        .map(|a| a.user)
        .unwrap_or_default();
    let target = state.users.lock().unwrap().get(&callee).cloned();
    let peer = match target {
        Some(u) => u,
        None => {
            info!("bye user not found: {}", callee);
            return tx.reply(rsip::StatusCode::NotFound).await;
        }
    };

    let mut inv_req = tx.original.clone();
    let via = tx.endpoint_inner.get_via(None, None)?;
    inv_req.headers.push_front(via.into());

    let key = TransactionKey::from_request(&inv_req, TransactionRole::Client)
        .expect("client_transaction");

    info!("Forwarding BYE to: {} -> {}", caller, peer.destination);

    let mut bye_tx = Transaction::new_client(key, inv_req, tx.endpoint_inner.clone(), None);
    bye_tx.destination = Some(peer.destination);

    bye_tx.send().await?;

    let dialog_id = DialogId::try_from(&bye_tx.original)?;
    if state.sessions.lock().unwrap().remove(&dialog_id) {
        info!("removed session: {}", dialog_id);
    }

    while let Some(msg) = bye_tx.receive().await {
        match msg {
            rsip::message::SipMessage::Response(mut resp) => {
                // pop first Via
                header_pop!(resp.headers, rsip::Header::Via);
                info!("UAC/BYE Forwarding response: {}", resp.to_string());
                tx.respond(resp).await?;
            }
            _ => {
                error!("UAC/BYE Received request: {}", msg.to_string());
            }
        }
    }

    Ok(())
}

async fn serve_index() -> impl IntoResponse {
    Html(include_str!("../assets/index.html"))
}
async fn serve_sip_js() -> impl IntoResponse {
    Html(include_str!("../assets/sip-0.17.1.js"))
}
fn create_http_app(state: Arc<AppState>) -> Router {
    Router::new()
        .route("/", get(serve_index))
        .route("/ws", get(websocket_handler))
        .route("/sip-0.17.1.js", get(serve_sip_js))
        .with_state(state)
        .layer(ServiceBuilder::new().layer(CorsLayer::permissive()))
}

async fn websocket_handler(
    ws: WebSocketUpgrade,
    State(state): State<Arc<AppState>>,
) -> impl IntoResponse {
    ws.protocols(["sip"])
        .on_upgrade(|socket| handle_websocket(socket, state))
}

async fn handle_websocket(socket: WebSocket, _state: Arc<AppState>) {
    let (mut ws_sink, mut ws_read) = socket.split();
    // let endpoint = _state.endpoint.clone();
    // let cancel_token = CancellationToken::new();
    // let (mut ws_sink, mut ws_read) = socket.split();
    // let transport_layer = _state.endpoint.transport_layer.inner.clone();
    // let sender = _state.sender.clone();
    // let local_addr = SipAddr {
    //     r#type: Some(rsip::transport::Transport::Ws),
    //     addr: rsip::HostWithPort {
    //         host: rsip::Host::Domain("localhost".to_string().into()),
    //         port: Some(8080.into()),
    //     },
    // };
    // let remote_addr = SipAddr {
    //     r#type: Some(rsip::transport::Transport::Ws),
    //     addr: rsip::HostWithPort {
    //         host: rsip::Host::Domain("localhost".to_string().into()),
    //         port: Some(8080.into()),
    //     },
    // };
    // // Create a new WebSocket connection
    // let connection = WebSocketConnection {
    //     inner: Arc::new(WebSocketInner {
    //         local_addr,
    //         remote_addr,
    //         ws_sink: Mutex::new(ws_sink.into()),
    //         ws_read: Mutex::new(Some(ws_read)),
    //     }),
    // };
    // let sip_connection = SipConnection::WebSocket(connection.clone());
    // let connection_addr = connection.get_addr().clone();

    // transport_layer.add_connection(sip_connection.clone());
    // info!(
    //     "Added WebSocket connection to transport layer: {}",
    //     connection_addr
    // );

    // if let Err(e) = sender.send(TransportEvent::New(sip_connection.clone())) {
    //     error!("Error sending new connection event: {:?}", e);
    //     return;
    // }

    // select! {
    //     _ = cancel_token.cancelled() => {
    //     }
    //     _ = connection.serve_loop(sender.clone()) => {
    //         info!(
    //             "WebSocket connection serve loop completed: {}",
    //             connection_addr
    //         );
    //     }
    // }

    // // Remove connection from transport layer when done
    // transport_layer.del_connection(&connection_addr);
    // info!(
    //     "Removed WebSocket connection from transport layer: {}",
    //     connection_addr
    // );

    // // Send connection closed event
    // if let Err(e) = sender.send(TransportEvent::Closed(sip_connection)) {
    //     warn!("Error sending WebSocket connection closed event: {:?}", e);
    // }
}

#[cfg(test)]
mod tests {
    use super::*;
    use rsipstack::transport::udp::UdpConnection;
    use rsipstack::{
        transport::{transport_layer::TransportConfig, TransportLayer},
        EndpointBuilder,
    };
    use std::sync::Arc;
    use tokio_util::sync::CancellationToken;

    #[tokio::test]
    async fn test_proxy_basic_functionality() {
        let _ = tracing_subscriber::fmt::try_init();

        // Create test proxy
        let token = CancellationToken::new();
        let transport_config = TransportConfig {
            tls: None,
            enable_ws: true,
            enable_wss: true,
        };
        let transport_layer = TransportLayer::with_config(token.clone(), transport_config);

        let udp_conn = UdpConnection::create_connection("127.0.0.1:0".parse().unwrap(), None)
            .await
            .unwrap();
        transport_layer.add_transport(udp_conn.into());

        let _endpoint = EndpointBuilder::new()
            .with_cancel_token(token.clone())
            .with_transport_layer(transport_layer)
            .build();

        // Just verify the endpoint was created successfully
        assert!(!token.is_cancelled());

        token.cancel();
        assert!(token.is_cancelled());
    }

    #[tokio::test]
    async fn test_proxy_state_management() {
        let endpoint = EndpointBuilder::new()
            .with_user_agent("MyApp/1.0")
            .with_timer_interval(Duration::from_millis(10))
            .with_allows(vec![rsip::Method::Invite, rsip::Method::Bye])
            .build();

        let state = Arc::new(AppState {
            users: Mutex::new(HashMap::new()),
            sessions: Mutex::new(HashSet::new()),
        });

        // Test user registration
        let user = User {
            username: "testuser".to_string(),
            destination: SipAddr {
                r#type: Some(rsip::transport::Transport::Udp),
                addr: rsip::HostWithPort {
                    host: rsip::Host::Domain("192.168.1.100".to_string().into()),
                    port: Some(5060.into()),
                },
            },
        };

        state
            .users
            .lock()
            .unwrap()
            .insert(user.username.clone(), user.clone());

        // Verify user is stored
        let stored_user = state.users.lock().unwrap().get("testuser").cloned();
        assert!(stored_user.is_some());
        assert_eq!(stored_user.unwrap().username, "testuser");
    }

    #[tokio::test]
    async fn test_dialog_session_management() {
        let endpoint = EndpointBuilder::new()
            .with_user_agent("MyApp/1.0")
            .with_timer_interval(Duration::from_millis(10))
            .with_allows(vec![rsip::Method::Invite, rsip::Method::Bye])
            .build();
        let state = Arc::new(AppState {
            users: Mutex::new(HashMap::new()),
            sessions: Mutex::new(HashSet::new()),
        });

        // Create a test dialog ID
        let dialog_id = DialogId {
            call_id: "test-call-id".to_string(),
            from_tag: "local-tag".to_string(),
            to_tag: "remote-tag".to_string(),
        };

        // Add session
        state.sessions.lock().unwrap().insert(dialog_id.clone());

        // Verify session is stored
        assert!(state.sessions.lock().unwrap().contains(&dialog_id));

        // Remove session
        assert!(state.sessions.lock().unwrap().remove(&dialog_id));
        assert!(!state.sessions.lock().unwrap().contains(&dialog_id));
    }

    #[tokio::test]
    async fn test_args_parsing() {
        use clap::Parser;

        // Test parsing all arguments
        let args = Args::try_parse_from(&[
            "proxy",
            "--port",
            "5060",
            "--tcp-port",
            "5060",
            "--ws-port",
            "8080",
            "--http-port",
            "8000",
        ])
        .unwrap();

        assert_eq!(args.port, 5060);
        assert_eq!(args.tcp_port, Some(5060));
        assert_eq!(args.ws_port, Some(8080));
        assert_eq!(args.http_port, Some(8000));
    }

    #[tokio::test]
    async fn test_transport_layer_integration() {
        let _ = tracing_subscriber::fmt::try_init();

        let token = CancellationToken::new();
        let transport_config = TransportConfig {
            tls: None,
            enable_ws: true,
            enable_wss: true,
        };
        let transport_layer = TransportLayer::with_config(token.clone(), transport_config);

        let (transport_tx, _transport_rx) = tokio::sync::mpsc::unbounded_channel();

        // Test that we can add different transport listeners
        let tcp_result = transport_layer
            .add_tcp_listener("127.0.0.1:0".parse().unwrap(), transport_tx.clone())
            .await;
        assert!(tcp_result.is_ok());

        let ws_result = transport_layer
            .add_ws_listener("127.0.0.1:0".parse().unwrap(), transport_tx.clone(), false)
            .await;
        assert!(ws_result.is_ok());

        token.cancel();
    }
}
