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
        .contacts()
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
        let buf = "REGISTER sips:bob@restsend.com SIP/2.0\r\n\r\n".as_bytes();
        test_conn.send_raw(buf, addr).await.expect("send_raw");
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
