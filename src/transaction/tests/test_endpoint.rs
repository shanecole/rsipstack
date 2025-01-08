use rsip::headers::*;
use std::time::Duration;
use tokio::{select, time::sleep};

#[tokio::test]
async fn test_endpoint_serve() {
    let endpoint = super::create_test_endpoint(None)
        .await
        .expect("create_test_endpoint");
    select! {
        _ = async {
            sleep(Duration::from_millis(10)).await;
            endpoint.shutdown();
            sleep(Duration::from_secs(1)).await;
        } => {
            assert!(false, "must not reach here");
        }
        _ = endpoint.serve()=> {}
    }
}

#[tokio::test]
async fn test_endpoint_recvrequests() {
    let addr = "127.0.0.1:0";
    let endpoint = super::create_test_endpoint(Some(addr))
        .await
        .expect("create_test_endpoint");

    let addr = endpoint
        .get_contacts()
        .get(0)
        .expect("must has connection")
        .to_owned();

    let send_loop = async {
        let test_conn = crate::transport::udp::UdpConnection::create_connection(
            "127.0.0.1:0".parse().unwrap(),
            None,
        )
        .await
        .expect("create_connection");
        let register_req = rsip::message::Request {
            method: rsip::method::Method::Register,
            uri: rsip::Uri {
                scheme: Some(rsip::Scheme::Sips),
                auth: Some(rsip::Auth {
                    user: "bob".to_string(),
                    password: None,
                }),
                host_with_port: rsip::HostWithPort::try_from("restsend.com")
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
        let buf: String = register_req.try_into().expect("try_into");
        test_conn
            .send_raw(&buf.as_bytes(), addr)
            .await
            .expect("send_raw");
        sleep(Duration::from_secs(1)).await;
    };

    let incoming_loop = async {
        let mut incoming = endpoint.incoming_transactions();
        incoming
            .recv()
            .await
            .unwrap()
            .expect("incoming")
            .original
            .clone()
    };

    select! {
        _ = send_loop => {
            assert!(false, "must not reach here");
        }
        _ = endpoint.serve()=> {}
        req = incoming_loop => {
            assert_eq!(req.method, rsip::method::Method::Register);
            assert_eq!(req.uri.to_string(), "sips:bob@restsend.com");
        }
    }
}
