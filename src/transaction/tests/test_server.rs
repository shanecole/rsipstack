use crate::transport::SipConnection;
use crate::{
    transport::{channel::ChannelConnection, connection::SipAddr, TransportEvent, TransportLayer},
    EndpointBuilder,
};
use rsip::headers::*;
use std::time::Duration;
use tokio::{select, sync::mpsc::unbounded_channel, time::sleep};
use tokio_util::sync::CancellationToken;
use tracing::info;

#[tokio::test]
async fn test_server_transaction() {
    let token = CancellationToken::new();
    let addr = SipAddr {
        r#type: Some(rsip::transport::Transport::Udp),
        addr: "127.0.0.1:2025".parse().expect("parse addr"),
    };
    let (incoming_tx, incoming_rx) = unbounded_channel();
    let (outgoing_tx, mut outgoing_rx) = unbounded_channel();

    let mock_conn: SipConnection =
        ChannelConnection::create_connection(incoming_rx, outgoing_tx, addr.clone())
            .await
            .expect("create_connection")
            .into();

    let tl = TransportLayer::new(token.child_token());
    tl.add_transport(mock_conn.clone());

    let endpoint = EndpointBuilder::new()
        .user_agent("rsipstack-test")
        .transport_layer(tl)
        .build();

    let addr = endpoint
        .get_contacts()
        .get(0)
        .expect("must has connection")
        .to_owned();

    let send_loop = async {
        let register_req = rsip::message::Request {
            method: rsip::method::Method::Register,
            uri: rsip::Uri {
                scheme: Some(rsip::Scheme::Sip),
                host_with_port: rsip::HostWithPort::try_from("127.0.0.1:2025")
                    .expect("host_port parse")
                    .into(),
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
        incoming_tx
            .send(TransportEvent::Incoming(
                register_req.into(),
                mock_conn.clone(),
                addr.clone(),
            ))
            .expect("incoming_tx.send");

        // wait 100 tring
        let resp_1xx = outgoing_rx.recv().await.expect("outgoing_rx");
        match resp_1xx {
            TransportEvent::Incoming(msg, _, _) => match msg {
                rsip::SipMessage::Response(resp) => {
                    info!("resp: {:?}", resp);
                    assert_eq!(resp.status_code, rsip::StatusCode::Trying);
                }
                _ => {
                    assert!(false, "unexpected message");
                }
            },
            _ => {
                assert!(false, "unexpected event");
            }
        };

        let must_200_resp = async {
            let resp_200 = outgoing_rx.recv().await.expect("outgoing_rx");
            match resp_200 {
                TransportEvent::Incoming(msg, _, _) => match msg {
                    rsip::SipMessage::Response(resp) => {
                        assert_eq!(resp.status_code, rsip::StatusCode::OK);
                    }
                    _ => {
                        assert!(false, "unexpected message");
                    }
                },
                _ => {
                    assert!(false, "unexpected event");
                }
            }
        };

        select! {
            _ = must_200_resp => {}
            _ = sleep(Duration::from_millis(500)) => {
                assert!(false, "timeout waiting");
            }
        };
    };

    let incoming_loop = async {
        let mut incoming = endpoint.incoming_transactions();
        let mut tx = incoming.recv().await.expect("incoming");
        assert_eq!(tx.original.method, rsip::method::Method::Register);
        let headers = tx.original.headers.clone();
        let done_response = rsip::Response {
            status_code: rsip::StatusCode::OK,
            version: rsip::Version::V2,
            headers,
            ..Default::default()
        };
        tx.send_trying().await.expect("send_trying");
        tx.respond(done_response).await.expect("respond 200");

        assert!(tx
            .endpoint_inner
            .finished_transactions
            .lock()
            .unwrap()
            .contains_key(&tx.key));
        sleep(Duration::from_secs(2)).await;
    };

    select! {
        _ = send_loop => {
        }
        _ = endpoint.serve()=> {}
        _ = incoming_loop => {
            assert!(false, "must not reach here");
        }
        _ = sleep(Duration::from_secs(1)) => {
            assert!(false, "timeout waiting");
        }
    }
}
