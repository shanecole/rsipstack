//! Simple SIP Connection Example
//!
//! This example demonstrates the most basic usage of rsipstack:
//! - Creating UDP/TCP connections
//! - Sending raw SIP messages
//! - Receiving responses
//! - Basic error handling
//!
//! Usage:
//! - UDP mode: cargo run --example simple_connection -- --transport udp
//! - TCP mode: cargo run --example simple_connection -- --transport tcp
//! - Server mode: cargo run --example simple_connection -- --mode server
//! - Client mode: cargo run --example simple_connection -- --mode client --target 127.0.0.1:5060

use clap::{Arg, Command};
use rsip::prelude::*; // Import HeadersExt trait for header access methods
use rsipstack::transport::{
    connection::TransportEvent,
    stream::StreamConnection, // Import StreamConnection trait
    tcp::TcpConnection,
    udp::UdpConnection,
    SipAddr,
    SipConnection,
};
use rsipstack::{Error, Result};
use std::time::Duration;
use tokio::sync::mpsc::unbounded_channel;
use tokio::{select, time::sleep};
use tokio_util::sync::CancellationToken;
use tracing::{error, info};

#[derive(Debug, Clone)]
enum TransportType {
    Udp,
    Tcp,
}

#[derive(Debug, Clone)]
enum Mode {
    Server,
    Client { target: String },
}

#[tokio::main]
async fn main() -> Result<()> {
    // Initialize logging
    tracing_subscriber::fmt()
        .with_max_level(tracing::Level::INFO)
        .init();

    // Parse command line arguments
    let matches = Command::new("simple_connection")
        .about("Simple SIP Connection Example")
        .arg(
            Arg::new("transport")
                .long("transport")
                .short('t')
                .value_name("TYPE")
                .help("Transport type (udp/tcp)")
                .default_value("udp"),
        )
        .arg(
            Arg::new("mode")
                .long("mode")
                .short('m')
                .value_name("MODE")
                .help("Mode (server/client)")
                .default_value("server"),
        )
        .arg(
            Arg::new("port")
                .long("port")
                .short('p')
                .value_name("PORT")
                .help("Local port")
                .default_value("5060"),
        )
        .arg(
            Arg::new("target")
                .long("target")
                .value_name("TARGET")
                .help("Target address for client mode")
                .default_value("127.0.0.1:5060"),
        )
        .get_matches();

    let transport_type = match matches.get_one::<String>("transport").unwrap().as_str() {
        "udp" => TransportType::Udp,
        "tcp" => TransportType::Tcp,
        _ => {
            error!("Invalid transport type. Use 'udp' or 'tcp'");
            return Err(Error::Error("invalid transport type".to_string()));
        }
    };

    let mode = match matches.get_one::<String>("mode").unwrap().as_str() {
        "server" => Mode::Server,
        "client" => Mode::Client {
            target: matches.get_one::<String>("target").unwrap().clone(),
        },
        _ => {
            error!("Invalid mode. Use 'server' or 'client'");
            return Err(Error::Error("invalid mode".to_string()));
        }
    };

    let port: u16 = matches
        .get_one::<String>("port")
        .unwrap()
        .parse()
        .map_err(|_| Error::Error("invalid port".to_string()))?;

    info!("Starting simple SIP connection example");
    info!(
        "Transport: {:?}, Mode: {:?}, Port: {}",
        transport_type, mode, port
    );

    match mode {
        Mode::Server => run_server(transport_type, port).await,
        Mode::Client { target } => run_client(transport_type, port, target).await,
    }
}

async fn run_server(transport_type: TransportType, port: u16) -> Result<()> {
    let local_addr = format!("127.0.0.1:{}", port)
        .parse()
        .expect("valid address");

    let connection: SipConnection = match transport_type {
        TransportType::Udp => {
            let conn = UdpConnection::create_connection(local_addr, None).await?;
            info!("Created UDP server on: {}", conn.get_addr());
            conn.into()
        }
        TransportType::Tcp => {
            // For TCP server, we need to create a listener
            let (_listener, sip_addr) = TcpConnection::create_listener(local_addr).await?;
            info!("Created TCP server on: {}", sip_addr);

            // For simplicity, we'll create a dummy UDP connection to get a SipConnection
            // In a real server, you'd handle TCP connections differently
            let udp_conn =
                UdpConnection::create_connection("127.0.0.1:0".parse().unwrap(), None).await?;
            udp_conn.into()
        }
    };

    let cancel_token = CancellationToken::new();
    let (sender, mut receiver) = unbounded_channel();

    info!("Server listening for incoming SIP messages...");
    info!("Send SIP messages to: {}", connection.get_addr());

    // Start the serve loop in a separate task
    let serve_connection = connection.clone();
    let serve_task = tokio::spawn(async move {
        if let Err(e) = serve_connection.serve_loop(sender).await {
            error!("Serve loop error: {}", e);
        }
    });

    loop {
        select! {
            _ = cancel_token.cancelled() => {
                info!("Server shutdown requested");
                break;
            }
            result = receiver.recv() => {
                match result {
                    Some(event) => {
                        match event {
                            TransportEvent::Incoming(message, from_connection, from_addr) => {
                                info!("Received message from: {}", from_addr);
                                info!("SIP Message: {}", message);

                                handle_sip_message(message, &from_connection, &from_addr).await?;
                            }
                            TransportEvent::New(new_connection) => {
                                info!("New connection: {}", new_connection);
                            }
                            TransportEvent::Closed(closed_connection) => {
                                info!("Connection closed: {}", closed_connection);
                            }
                        }
                    }
                    None => {
                        info!("Receiver closed");
                        break;
                    }
                }
            }
        }
    }

    serve_task.abort();
    Ok(())
}

async fn handle_sip_message(
    sip_msg: rsip::message::SipMessage,
    connection: &SipConnection,
    from_addr: &SipAddr,
) -> Result<()> {
    match sip_msg {
        rsip::message::SipMessage::Request(req) => {
            info!("Handling {} request", req.method);

            // Create appropriate response
            let response = match req.method {
                rsip::Method::Options => create_options_response(&req),
                rsip::Method::Message => create_message_response(&req),
                rsip::Method::Register => create_register_response(&req),
                _ => create_not_implemented_response(&req),
            };

            // Send response
            connection.send(response.into(), Some(from_addr)).await?;
            info!("Sent response for {} request", req.method);
        }
        rsip::message::SipMessage::Response(resp) => {
            info!("Received {} response", resp.status_code);
        }
    }

    Ok(())
}

fn create_options_response(req: &rsip::Request) -> rsip::Response {
    rsip::Response {
        status_code: rsip::StatusCode::OK,
        headers: rsip::Headers::from(vec![
            rsip::Header::Via(req.via_header().unwrap().clone()),
            rsip::Header::From(req.from_header().unwrap().clone()),
            rsip::Header::To(req.to_header().unwrap().clone()),
            rsip::Header::CallId(req.call_id_header().unwrap().clone()),
            rsip::Header::CSeq(req.cseq_header().unwrap().clone()),
            rsip::Header::Allow("INVITE, ACK, CANCEL, OPTIONS, BYE, REGISTER, MESSAGE".into()),
            rsip::Header::UserAgent("rsipstack-simple-connection/0.1".into()),
            rsip::Header::ContentLength(rsip::headers::ContentLength::from(0)),
        ]),
        body: Default::default(),
        version: rsip::Version::V2,
    }
}

fn create_message_response(req: &rsip::Request) -> rsip::Response {
    rsip::Response {
        status_code: rsip::StatusCode::OK,
        headers: rsip::Headers::from(vec![
            rsip::Header::Via(req.via_header().unwrap().clone()),
            rsip::Header::From(req.from_header().unwrap().clone()),
            rsip::Header::To(req.to_header().unwrap().clone()),
            rsip::Header::CallId(req.call_id_header().unwrap().clone()),
            rsip::Header::CSeq(req.cseq_header().unwrap().clone()),
            rsip::Header::ContentLength(rsip::headers::ContentLength::from(0)),
        ]),
        body: Default::default(),
        version: rsip::Version::V2,
    }
}

fn create_register_response(req: &rsip::Request) -> rsip::Response {
    rsip::Response {
        status_code: rsip::StatusCode::OK,
        headers: rsip::Headers::from(vec![
            rsip::Header::Via(req.via_header().unwrap().clone()),
            rsip::Header::From(req.from_header().unwrap().clone()),
            rsip::Header::To(req.to_header().unwrap().clone()),
            rsip::Header::CallId(req.call_id_header().unwrap().clone()),
            rsip::Header::CSeq(req.cseq_header().unwrap().clone()),
            rsip::Header::Contact("<sip:server@127.0.0.1:5060>;expires=3600".into()),
            rsip::Header::ContentLength(rsip::headers::ContentLength::from(0)),
        ]),
        body: Default::default(),
        version: rsip::Version::V2,
    }
}

fn create_not_implemented_response(req: &rsip::Request) -> rsip::Response {
    rsip::Response {
        status_code: rsip::StatusCode::NotImplemented,
        headers: rsip::Headers::from(vec![
            rsip::Header::Via(req.via_header().unwrap().clone()),
            rsip::Header::From(req.from_header().unwrap().clone()),
            rsip::Header::To(req.to_header().unwrap().clone()),
            rsip::Header::CallId(req.call_id_header().unwrap().clone()),
            rsip::Header::CSeq(req.cseq_header().unwrap().clone()),
            rsip::Header::ContentLength(rsip::headers::ContentLength::from(0)),
        ]),
        body: Default::default(),
        version: rsip::Version::V2,
    }
}

async fn run_client(transport_type: TransportType, local_port: u16, target: String) -> Result<()> {
    let local_addr = format!("127.0.0.1:{}", local_port)
        .parse()
        .expect("valid address");
    let target_addr: std::net::SocketAddr = target.parse().expect("valid target address");

    let connection: SipConnection = match transport_type {
        TransportType::Udp => {
            let conn = UdpConnection::create_connection(local_addr, None).await?;
            info!("Created UDP client on: {}", conn.get_addr());
            conn.into()
        }
        TransportType::Tcp => {
            // For TCP client, connect to remote address
            let target_sip_addr = SipAddr {
                r#type: Some(rsip::transport::Transport::Tcp),
                addr: target_addr.into(),
            };
            let conn = TcpConnection::connect(&target_sip_addr).await?;
            info!("Created TCP client on: {}", conn.get_addr());
            conn.into()
        }
    };

    let sip_target = SipAddr {
        r#type: Some(match transport_type {
            TransportType::Udp => rsip::transport::Transport::Udp,
            TransportType::Tcp => rsip::transport::Transport::Tcp,
        }),
        addr: target_addr.into(),
    };

    info!("Sending test messages to: {}", target);

    // Send OPTIONS request
    send_options_request(&connection, &sip_target).await?;
    sleep(Duration::from_millis(500)).await;

    // Send MESSAGE request
    send_message_request(&connection, &sip_target, "Hello from rsipstack!").await?;
    sleep(Duration::from_millis(500)).await;

    // Send REGISTER request
    send_register_request(&connection, &sip_target).await?;
    sleep(Duration::from_millis(500)).await;

    info!("All test messages sent successfully!");

    Ok(())
}

async fn send_options_request(connection: &SipConnection, target: &SipAddr) -> Result<()> {
    let request = create_options_request(target, connection.get_addr())?;

    info!("Sending OPTIONS request: {}", request);
    connection.send(request.into(), Some(target)).await?;

    Ok(())
}

async fn send_message_request(
    connection: &SipConnection,
    target: &SipAddr,
    message: &str,
) -> Result<()> {
    let request = create_message_request(target, connection.get_addr(), message)?;

    info!("Sending MESSAGE request: {}", request);
    connection.send(request.into(), Some(target)).await?;

    Ok(())
}

async fn send_register_request(connection: &SipConnection, target: &SipAddr) -> Result<()> {
    let request = create_register_request(target, connection.get_addr())?;

    info!("Sending REGISTER request: {}", request);
    connection.send(request.into(), Some(target)).await?;

    Ok(())
}

fn create_options_request(target: &SipAddr, local: &SipAddr) -> Result<rsip::Request> {
    let uri = rsip::Uri {
        scheme: Some(rsip::Scheme::Sip),
        auth: None,
        host_with_port: target.addr.clone(),
        params: Default::default(),
        headers: Default::default(),
    };

    Ok(rsip::Request {
        method: rsip::Method::Options,
        uri,
        version: rsip::Version::V2,
        headers: rsip::Headers::from(vec![
            rsip::Header::Via(
                format!(
                    "SIP/2.0/{} {};branch=z9hG4bK{}",
                    match target.r#type.unwrap_or(rsip::transport::Transport::Udp) {
                        rsip::transport::Transport::Udp => "UDP",
                        rsip::transport::Transport::Tcp => "TCP",
                        _ => "UDP",
                    },
                    local.addr,
                    generate_branch()
                )
                .into(),
            ),
            rsip::Header::From(
                format!("<sip:client@{}>;tag={}", local.addr, generate_tag()).into(),
            ),
            rsip::Header::To(format!("<sip:server@{}>", target.addr).into()),
            rsip::Header::CallId(generate_call_id().into()),
            rsip::Header::CSeq(format!("1 OPTIONS").into()),
            rsip::Header::UserAgent("rsipstack-simple-connection/0.1".into()),
            rsip::Header::ContentLength(rsip::headers::ContentLength::from(0)),
        ]),
        body: Default::default(),
    })
}

fn create_message_request(
    target: &SipAddr,
    local: &SipAddr,
    message: &str,
) -> Result<rsip::Request> {
    let uri = rsip::Uri {
        scheme: Some(rsip::Scheme::Sip),
        auth: None,
        host_with_port: target.addr.clone(),
        params: Default::default(),
        headers: Default::default(),
    };

    Ok(rsip::Request {
        method: rsip::Method::Message,
        uri,
        version: rsip::Version::V2,
        headers: rsip::Headers::from(vec![
            rsip::Header::Via(
                format!(
                    "SIP/2.0/{} {};branch=z9hG4bK{}",
                    match target.r#type.unwrap_or(rsip::transport::Transport::Udp) {
                        rsip::transport::Transport::Udp => "UDP",
                        rsip::transport::Transport::Tcp => "TCP",
                        _ => "UDP",
                    },
                    local.addr,
                    generate_branch()
                )
                .into(),
            ),
            rsip::Header::From(
                format!("<sip:client@{}>;tag={}", local.addr, generate_tag()).into(),
            ),
            rsip::Header::To(format!("<sip:server@{}>", target.addr).into()),
            rsip::Header::CallId(generate_call_id().into()),
            rsip::Header::CSeq(format!("1 MESSAGE").into()),
            rsip::Header::ContentType("text/plain".into()),
            rsip::Header::ContentLength(rsip::headers::ContentLength::from(message.len() as u32)),
        ]),
        body: message.as_bytes().to_vec().into(),
    })
}

fn create_register_request(target: &SipAddr, local: &SipAddr) -> Result<rsip::Request> {
    let uri = rsip::Uri {
        scheme: Some(rsip::Scheme::Sip),
        auth: None,
        host_with_port: target.addr.clone(),
        params: Default::default(),
        headers: Default::default(),
    };

    Ok(rsip::Request {
        method: rsip::Method::Register,
        uri,
        version: rsip::Version::V2,
        headers: rsip::Headers::from(vec![
            rsip::Header::Via(
                format!(
                    "SIP/2.0/{} {};branch=z9hG4bK{}",
                    match target.r#type.unwrap_or(rsip::transport::Transport::Udp) {
                        rsip::transport::Transport::Udp => "UDP",
                        rsip::transport::Transport::Tcp => "TCP",
                        _ => "UDP",
                    },
                    local.addr,
                    generate_branch()
                )
                .into(),
            ),
            rsip::Header::From(
                format!("<sip:client@{}>;tag={}", local.addr, generate_tag()).into(),
            ),
            rsip::Header::To(format!("<sip:client@{}>", local.addr).into()),
            rsip::Header::CallId(generate_call_id().into()),
            rsip::Header::CSeq(format!("1 REGISTER").into()),
            rsip::Header::Contact(format!("<sip:client@{}>;expires=3600", local.addr).into()),
            rsip::Header::ContentLength(rsip::headers::ContentLength::from(0)),
        ]),
        body: Default::default(),
    })
}

fn generate_branch() -> String {
    use rand::Rng;
    let mut rng = rand::rng();
    format!("{:08x}", rng.random::<u32>())
}

fn generate_tag() -> String {
    use rand::Rng;
    let mut rng = rand::rng();
    format!("{}", rng.random::<u32>())
}

fn generate_call_id() -> String {
    use rand::Rng;
    let mut rng = rand::rng();
    format!("call-id-{}", rng.random::<u64>())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_udp_connection_creation() {
        let local_addr = "127.0.0.1:0".parse().expect("valid address");
        let connection = UdpConnection::create_connection(local_addr, None).await;
        assert!(connection.is_ok());

        let conn = connection.unwrap();
        assert!(conn.get_addr().addr.port.is_some());
        assert_eq!(
            conn.get_addr().r#type,
            Some(rsip::transport::Transport::Udp)
        );
    }

    #[tokio::test]
    async fn test_tcp_listener_creation() {
        let local_addr = "127.0.0.1:0".parse().expect("valid address");
        let listener_result = TcpConnection::create_listener(local_addr).await;
        assert!(listener_result.is_ok());

        let (_listener, addr) = listener_result.unwrap();
        assert!(addr.addr.port.is_some());
        assert_eq!(addr.r#type, Some(rsip::transport::Transport::Tcp));
    }

    #[tokio::test]
    async fn test_message_creation() {
        let local_addr: std::net::SocketAddr = "127.0.0.1:5060".parse().expect("valid address");
        let target_addr = SipAddr {
            r#type: Some(rsip::transport::Transport::Udp),
            addr: "127.0.0.1:5061"
                .parse::<std::net::SocketAddr>()
                .expect("valid address")
                .into(),
        };

        let request = create_options_request(
            &target_addr,
            &SipAddr {
                r#type: Some(rsip::transport::Transport::Udp),
                addr: local_addr.into(),
            },
        )
        .unwrap();

        // Verify the message can be parsed
        let parsed = rsip::message::SipMessage::try_from(request.to_string().as_str());
        assert!(parsed.is_ok());

        if let Ok(rsip::message::SipMessage::Request(req)) = parsed {
            assert_eq!(req.method, rsip::method::Method::Options);
        }
    }

    #[tokio::test]
    async fn test_rfc3261_options_compliance() {
        let local_addr: std::net::SocketAddr = "127.0.0.1:5060".parse().expect("valid address");
        let target_addr = SipAddr {
            r#type: Some(rsip::transport::Transport::Udp),
            addr: "127.0.0.1:5061"
                .parse::<std::net::SocketAddr>()
                .expect("valid address")
                .into(),
        };

        let request = create_options_request(
            &target_addr,
            &SipAddr {
                r#type: Some(rsip::transport::Transport::Udp),
                addr: local_addr.into(),
            },
        )
        .unwrap();

        // Test RFC 3261 compliance
        assert_eq!(request.method, rsip::Method::Options);
        assert_eq!(request.version, rsip::Version::V2);

        // Must have Via header
        let via = request.via_header().unwrap();
        assert!(via.to_string().contains("SIP/2.0/UDP"));
        assert!(via.to_string().contains("branch=z9hG4bK")); // RFC 3261 magic cookie

        // Must have From header with tag
        let from = request.from_header().unwrap();
        assert!(from.to_string().contains("tag="));

        // Must have To header
        let to = request.to_header().unwrap();
        assert!(!to.to_string().is_empty());

        // Must have Call-ID
        let call_id = request.call_id_header().unwrap();
        assert!(!call_id.to_string().is_empty());

        // Must have CSeq
        let cseq = request.cseq_header().unwrap();
        assert!(cseq.to_string().contains("OPTIONS"));

        // Check Content-Length header exists in headers (without direct accessor)
        let headers = &request.headers;
        let has_content_length = headers
            .iter()
            .any(|h| matches!(h, rsip::Header::ContentLength(_)));
        assert!(has_content_length, "Must have Content-Length header");
    }

    #[tokio::test]
    async fn test_rfc3261_message_compliance() {
        let local_addr: std::net::SocketAddr = "127.0.0.1:5060".parse().expect("valid address");
        let target_addr = SipAddr {
            r#type: Some(rsip::transport::Transport::Udp),
            addr: "127.0.0.1:5061"
                .parse::<std::net::SocketAddr>()
                .expect("valid address")
                .into(),
        };

        let message_content = "Hello World!";
        let request = create_message_request(
            &target_addr,
            &SipAddr {
                r#type: Some(rsip::transport::Transport::Udp),
                addr: local_addr.into(),
            },
            message_content,
        )
        .unwrap();

        // Test RFC 3261 compliance
        assert_eq!(request.method, rsip::Method::Message);

        // Check Content-Type header exists in headers
        let headers = &request.headers;
        let has_content_type = headers
            .iter()
            .any(|h| matches!(h, rsip::Header::ContentType(_)));
        assert!(has_content_type, "MESSAGE must have Content-Type header");

        // Check Content-Length header exists in headers
        let has_content_length = headers
            .iter()
            .any(|h| matches!(h, rsip::Header::ContentLength(_)));
        assert!(has_content_length, "Must have Content-Length header");

        // Body content
        assert_eq!(
            std::str::from_utf8(request.body()).unwrap(),
            message_content
        );
    }

    #[tokio::test]
    async fn test_rfc3261_register_compliance() {
        let local_addr: std::net::SocketAddr = "127.0.0.1:5060".parse().expect("valid address");
        let target_addr = SipAddr {
            r#type: Some(rsip::transport::Transport::Udp),
            addr: "127.0.0.1:5061"
                .parse::<std::net::SocketAddr>()
                .expect("valid address")
                .into(),
        };

        let request = create_register_request(
            &target_addr,
            &SipAddr {
                r#type: Some(rsip::transport::Transport::Udp),
                addr: local_addr.into(),
            },
        )
        .unwrap();

        // Test RFC 3261 compliance
        assert_eq!(request.method, rsip::Method::Register);

        // Must have Contact header
        let contact = request.contact_header().unwrap();
        assert!(contact.to_string().contains("expires=3600"));

        // To and From should be the same for REGISTER
        let from = request.from_header().unwrap();
        let to = request.to_header().unwrap();
        // Extract URI parts for comparison (ignore tags)
        assert!(from.to_string().contains("client@"));
        assert!(to.to_string().contains("client@"));
    }

    #[tokio::test]
    async fn test_response_creation_rfc3261() {
        let local_addr: std::net::SocketAddr = "127.0.0.1:5060".parse().expect("valid address");
        let target_addr = SipAddr {
            r#type: Some(rsip::transport::Transport::Udp),
            addr: "127.0.0.1:5061"
                .parse::<std::net::SocketAddr>()
                .expect("valid address")
                .into(),
        };

        let request = create_options_request(
            &target_addr,
            &SipAddr {
                r#type: Some(rsip::transport::Transport::Udp),
                addr: local_addr.into(),
            },
        )
        .unwrap();

        let response = create_options_response(&request);

        // Test RFC 3261 response compliance
        assert_eq!(response.status_code, rsip::StatusCode::OK);
        assert_eq!(response.version, rsip::Version::V2);

        // Response must copy Via, From, To, Call-ID, CSeq from request
        assert_eq!(
            response.via_header().unwrap().to_string(),
            request.via_header().unwrap().to_string()
        );
        assert_eq!(
            response.from_header().unwrap().to_string(),
            request.from_header().unwrap().to_string()
        );
        assert_eq!(
            response.to_header().unwrap().to_string(),
            request.to_header().unwrap().to_string()
        );
        assert_eq!(
            response.call_id_header().unwrap().to_string(),
            request.call_id_header().unwrap().to_string()
        );
        assert_eq!(
            response.cseq_header().unwrap().to_string(),
            request.cseq_header().unwrap().to_string()
        );

        // Check Allow header exists in headers
        let headers = &response.headers;
        let has_allow = headers.iter().any(|h| matches!(h, rsip::Header::Allow(_)));
        assert!(has_allow, "OPTIONS response should have Allow header");
    }
}
