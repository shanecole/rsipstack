use crate::transaction::key::{TransactionKey, TransactionRole};
use crate::transaction::transaction::Transaction;
use crate::transport::udp::UdpConnection;
use crate::transport::SipAddr;
use crate::{transport::TransportEvent, Result};
use rsip::{headers::*, Header, Response, SipMessage, Uri};
use std::convert::TryFrom;
use std::time::Duration;
use tokio::{select, sync::mpsc::unbounded_channel, time::sleep};
use tracing::info;

#[tokio::test]
async fn test_client_transaction() -> Result<()> {
    let endpoint = super::create_test_endpoint(Some("127.0.0.1:0")).await?;
    let server_addr = endpoint.get_addrs().first().expect("must has connection").to_owned();
    info!("server addr: {}", server_addr);

    let peer_server = UdpConnection::create_connection("127.0.0.1:0".parse()?, None, None).await?;
    let peer_server_loop = async {
        let (sender, mut recevier) = unbounded_channel();
        select! {
            _ = async {
                if let Some(event) = recevier.recv().await {
                    if let TransportEvent::Incoming(msg, connection, _) = event {
                        info!("recv request: {}", msg);
                        assert!(msg.is_request());
                        match msg {
                            SipMessage::Request(req) => {
                                let headers = req.headers.clone();
                                let response = SipMessage::Response(rsip::message::Response {
                                    version: rsip::Version::V2,
                                    status_code:rsip::StatusCode::Trying,
                                    headers: headers.clone(),
                                    body: Default::default(),
                                });
                                connection.send(response, None).await.expect("send trying");
                                sleep(Duration::from_millis(100)).await;

                                let response = SipMessage::Response(rsip::message::Response {
                                    version: rsip::Version::V2,
                                    status_code:rsip::StatusCode::OK,
                                    headers,
                                    body: Default::default(),
                                });
                                connection.send(response, None).await.expect("send Ok");
                                sleep(Duration::from_secs(1)).await;
                            }
                            _ => {
                                panic!( "must not reach here");
                            }
                        }
                    }
                } else {
                    panic!( "must not reach here");
                }
            } => {}
            _ = peer_server.serve_loop(sender) => {
                panic!( "must not reach here");
            }
        }
    };

    let recv_loop = async {
        let register_req = rsip::message::Request {
            method: rsip::method::Method::Register,
            uri: rsip::Uri {
                scheme: Some(rsip::Scheme::Sip),
                host_with_port: peer_server.get_addr().addr.clone(),
                ..Default::default()
            },
            headers: vec![
                Via::new("SIP/2.0/TLS restsend.com:5061;branch=z9hG4bKnashd92").into(),
                CSeq::new("1 REGISTER").into(),
                From::new("Bob <sips:bob@restsend.com>;tag=ja743ks76zlflH").into(),
                CallId::new("1j9FpLxk3uxtm8tn@restsend.com").into(),
            ]
            .into(),
            version: rsip::Version::V2,
            body: Default::default(),
        };

        let key = TransactionKey::from_request(&register_req, TransactionRole::Client).expect("client_transaction");
        let mut tx = Transaction::new_client(key, register_req, endpoint.inner.clone(), None);
        tx.send().await.expect("send request");

        while let Some(resp) = tx.receive().await {
            info!("Received response: {:?}", resp);
        }
    };

    select! {
        _ = recv_loop => {}
        _ = peer_server_loop => {
            panic!( "must not reach here");
        }
        _ = endpoint.serve() => {
            panic!( "must not reach here");
        }
        _ = sleep(Duration::from_secs(1)) => {
            panic!( "timeout waiting");
        }
    }
    Ok(())
}

#[tokio::test]
async fn test_make_ack_uses_contact_and_reversed_route_order() -> Result<()> {
    let endpoint = super::create_test_endpoint(None).await?;

    let raw_response = "SIP/2.0 200 OK\r\n\
Via: SIP/2.0/TCP uac.example.com:5060;branch=z9hG4bK1\r\n\
Record-Route: <sip:proxy1.example.com:5060;transport=tcp;lr>\r\n\
Record-Route: <sip:proxy2.example.com:5070;transport=tcp;lr>\r\n\
From: <sip:alice@example.com>;tag=from-tag\r\n\
To: <sip:bob@example.com>;tag=to-tag\r\n\
Call-ID: callid@example.com\r\n\
CSeq: 1 INVITE\r\n\
Contact: <sip:uas@192.0.2.55:5080;transport=tcp>\r\n\
Content-Length: 0\r\n\r\n";

    let response = Response::try_from(raw_response)?;

    let ack = endpoint.inner.make_ack(&response, None, None)?;

    let expected_uri = Uri::try_from("sip:uas@192.0.2.55:5080;transport=tcp")?;
    assert_eq!(ack.uri, expected_uri, "ACK must target the remote Contact");

    // Check Content-Length
    let content_length: String = ack
        .headers
        .iter()
        .filter_map(|header| match header {
            Header::ContentLength(content_length) => Some(content_length.value().to_string()),
            _ => None,
        })
        .next()
        .expect("ACK must include a Content-Length header");

    assert_eq!(content_length, "0", "Content-Length of ACK must be 0");
    let routes: Vec<String> = ack
        .headers
        .iter()
        .filter_map(|header| match header {
            Header::Route(route) => Some(route.value().to_string()),
            _ => None,
        })
        .collect();

    assert_eq!(
        routes,
        vec!["<sip:proxy2.example.com:5070;transport=tcp;lr>".to_string(), "<sip:proxy1.example.com:5060;transport=tcp;lr>".to_string()],
        "ACK Route headers must follow the reversed Record-Route order"
    );

    Ok(())
}

#[tokio::test]
async fn test_client_invite_sends_ack_for_non_2xx() -> Result<()> {
    use tokio::time::timeout;

    // Initialize tracing for debugging
    let _ = tracing_subscriber::fmt().with_max_level(tracing::Level::DEBUG).with_test_writer().try_init();

    let endpoint = super::create_test_endpoint(Some("127.0.0.1:0")).await?;
    let server_addr = endpoint.get_addrs().first().expect("must have connection").to_owned();
    info!("server addr: {}", server_addr);

    // Start endpoint serving to process incoming messages
    let endpoint_inner = endpoint.inner.clone();
    tokio::spawn(async move {
        endpoint_inner.serve().await.ok();
    });

    // Create a mock peer that will send a non-2xx response
    let peer_server = UdpConnection::create_connection("127.0.0.1:0".parse()?, None, None).await?;
    let peer_addr = peer_server.get_addr().clone();
    info!("peer addr: {}", peer_addr);

    let (sender, mut receiver) = unbounded_channel();
    tokio::spawn(async move {
        peer_server.serve_loop(sender).await.ok();
    });

    let peer_server_loop = async {
        let mut received_invite = false;
        let mut received_ack = false;

        loop {
            if let Ok(Some(event)) = timeout(Duration::from_secs(5), receiver.recv()).await {
                if let TransportEvent::Incoming(msg, connection, _) = event {
                    if let SipMessage::Request(req) = msg {
                        info!("peer received request: {}", req.method);
                        if req.method == rsip::Method::Invite {
                            received_invite = true;
                            // Send 486 Busy Here response
                            let response = rsip::Response {
                                status_code: rsip::StatusCode::BusyHere,
                                headers: req.headers.clone(),
                                version: rsip::Version::V2,
                                body: vec![],
                            };
                            info!("peer sending 486 Busy Here");
                            connection.send(response.into(), None).await.ok();
                        } else if req.method == rsip::Method::Ack {
                            info!("peer received ACK - test success!");
                            received_ack = true;
                            break;
                        }
                    }
                }
            } else {
                break;
            }
        }

        (received_invite, received_ack)
    };

    let client_loop = async {
        // Create INVITE request
        let invite_req = rsip::Request {
            method: rsip::Method::Invite,
            uri: Uri::try_from("sip:bob@example.com").unwrap(),
            version: rsip::Version::V2,
            headers: rsip::Headers::from(vec![
                Via::new("SIP/2.0/UDP test.example.com:5060;branch=z9hG4bKtest-ack").into(),
                From::new("sip:alice@example.com").with_tag("from-tag".into()).unwrap().into(),
                To::new("sip:bob@example.com").into(),
                CallId::new("test-call-id@example.com").into(),
                CSeq::new("1 INVITE").into(),
                MaxForwards::new("70").into(),
            ]),
            body: vec![],
        };

        let key = TransactionKey::from_request(&invite_req, TransactionRole::Client)?;

        let mut client_tx = Transaction::new_client(key, invite_req.clone(), endpoint.inner.clone(), None);

        // Set destination to peer
        client_tx.destination = Some(peer_addr);

        info!("client sending INVITE");
        client_tx.send().await?;

        // Wait for response
        if let Ok(Some(msg)) = timeout(Duration::from_secs(5), client_tx.receive()).await
            && let rsip::SipMessage::Response(resp) = msg {
                info!("client received response: {}", resp.status_code);
                assert_eq!(resp.status_code, rsip::StatusCode::BusyHere);
            }

        // Give some time for ACK to be sent
        tokio::time::sleep(Duration::from_millis(100)).await;

        Result::<()>::Ok(())
    };

    let (peer_result, client_result) = tokio::join!(peer_server_loop, client_loop);
    client_result?;

    let (received_invite, received_ack) = peer_result;
    assert!(received_invite, "Peer should have received INVITE");
    assert!(received_ack, "Peer should have received ACK for non-2xx response");

    Ok(())
}

#[tokio::test]
async fn test_make_ack_uses_contact_with_ob() -> Result<()> {
    let endpoint = super::create_test_endpoint(None).await?;

    let raw_response = "SIP/2.0 200 OK\r\n\
Via: SIP/2.0/TCP uac.example.com:5060;branch=z9hG4bK1;rport=15060;received=1.2.3.4;\r\n\
From: <sip:alice@example.com>;tag=from-tag\r\n\
To: <sip:bob@example.com>;tag=to-tag\r\n\
Call-ID: callid@example.com\r\n\
CSeq: 1 INVITE\r\n\
Contact: <sip:uas@192.0.2.55:5080;ob>\r\n\
Content-Length: 0\r\n\r\n";

    let response = Response::try_from(raw_response)?;
    let dest = SipAddr {
        r#type: Some(rsip::transport::Transport::Tcp),
        addr: "1.2.3.4:15060".try_into()?,
    };
    let ack = endpoint.inner.make_ack(&response, None, Some(&dest))?;
    let expected_uri = Uri::try_from("sip:uas@1.2.3.4:15060;transport=tcp")?;
    assert_eq!(ack.uri, expected_uri, "ACK must target the remote Contact");
    Ok(())
}
