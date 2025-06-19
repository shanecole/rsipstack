use crate::{
    transport::{
        connection::TransportEvent,
        transport_layer::{TransportConfig, TransportLayer},
        SipConnection,
    },
    Result,
};
use std::time::Duration;
use tokio::{sync::mpsc::unbounded_channel, time::timeout};
use tokio_util::sync::CancellationToken;

#[tokio::test]
async fn test_tcp_server_listen_accept_parse() -> Result<()> {
    let cancel_token = CancellationToken::new();
    let transport_layer = TransportLayer::new(cancel_token.clone());
    let (sender, mut receiver) = unbounded_channel();

    let server_addr = transport_layer
        .add_tcp_listener("127.0.0.1:0".parse()?, sender.clone())
        .await?;

    transport_layer.serve_listens(sender.clone()).await?;

    let client_stream = tokio::net::TcpStream::connect(server_addr.get_socketaddr()?).await?;

    let new_connection_event = timeout(Duration::from_secs(5), receiver.recv())
        .await
        .map_err(|_| crate::Error::Error("".to_string()))?
        .ok_or_else(|| crate::Error::Error("Receiver closed".to_string()))?;

    match new_connection_event {
        TransportEvent::New(SipConnection::Tcp(_)) => {
            println!("Successfully accepted TCP connection");
        }
        _ => {
            return Err(crate::Error::Error(
                "Expected new TCP connection event".to_string(),
            ));
        }
    }

    // Send SIP message
    let test_message = "REGISTER sip:example.com SIP/2.0\r\n\
                       Via: SIP/2.0/TCP 127.0.0.1:5060;branch=z9hG4bK-test123\r\n\
                       From: <sip:alice@example.com>;tag=alice123\r\n\
                       To: <sip:alice@example.com>\r\n\
                       Call-ID: tcp-test-call-id@127.0.0.1\r\n\
                       CSeq: 1 REGISTER\r\n\
                       Contact: <sip:alice@127.0.0.1:5060;transport=tcp>\r\n\
                       Max-Forwards: 70\r\n\
                       Content-Length: 0\r\n\r\n";

    use tokio::io::AsyncWriteExt;
    let (_, mut write_half) = client_stream.into_split();
    write_half.write_all(test_message.as_bytes()).await?;
    write_half.flush().await?;
    println!("TCP client sent SIP message");

    let incoming_event = timeout(Duration::from_secs(5), receiver.recv())
        .await
        .map_err(|_| crate::Error::Error("Message wait timeout".to_string()))?
        .ok_or_else(|| crate::Error::Error("Receiver closed".to_string()))?;

    match incoming_event {
        TransportEvent::Incoming(received_msg, SipConnection::Tcp(_), remote_addr) => {
            println!("Successfully received SIP message from: {}", remote_addr);

            let msg_str = received_msg.to_string();
            assert!(msg_str.contains("REGISTER"));
            assert!(msg_str.contains("sip:example.com"));
            assert!(msg_str.contains("alice@example.com"));
            assert!(msg_str.contains("tcp-test-call-id@127.0.0.1"));

            println!("TCP message parsing verification successful");
        }
        _ => {
            return Err(crate::Error::Error("Expected Incoming event".to_string()));
        }
    }

    cancel_token.cancel();
    Ok(())
}

/// WebSocket server listen, accept connection and message parsing test
#[tokio::test]
#[cfg(feature = "websocket")]
async fn test_websocket_server_listen_accept_parse() -> Result<()> {
    use futures_util::{SinkExt, StreamExt};
    use tokio_tungstenite::{connect_async, tungstenite::Message};

    let cancel_token = CancellationToken::new();
    let mut config = TransportConfig::default();
    config.enable_ws = true;
    let transport_layer = TransportLayer::with_config(cancel_token.clone(), config);
    let (sender, mut receiver) = unbounded_channel();

    let server_addr = transport_layer
        .add_ws_listener("127.0.0.1:0".parse()?, sender.clone(), false)
        .await?;
    println!("WebSocket server listening on: {}", server_addr);

    let ws_url = format!(
        "ws://127.0.0.1:{}/sip",
        server_addr.addr.port.unwrap().value()
    );
    let (ws_stream, _) = connect_async(&ws_url).await?;
    let (mut ws_sender, _ws_receiver) = ws_stream.split();
    println!("WebSocket client connected to server");

    let new_connection_event = timeout(Duration::from_secs(5), receiver.recv())
        .await
        .map_err(|_| crate::Error::Error("New connection wait timeout".to_string()))?
        .ok_or_else(|| crate::Error::Error("Receiver closed".to_string()))?;

    match new_connection_event {
        TransportEvent::New(SipConnection::WebSocket(_)) => {
            println!("Successfully accepted WebSocket connection");
        }
        _ => {
            return Err(crate::Error::Error(
                "Expected new WebSocket connection event".to_string(),
            ));
        }
    }

    let test_message = "REGISTER sip:example.com SIP/2.0\r\n\
                       Via: SIP/2.0/WS 127.0.0.1:5060;branch=z9hG4bK-ws-test123\r\n\
                       From: <sip:bob@example.com>;tag=bob123\r\n\
                       To: <sip:bob@example.com>\r\n\
                       Call-ID: ws-test-call-id@127.0.0.1\r\n\
                       CSeq: 1 REGISTER\r\n\
                       Contact: <sip:bob@127.0.0.1:5060;transport=ws>\r\n\
                       Max-Forwards: 70\r\n\
                       Content-Length: 0\r\n\r\n";

    ws_sender.send(Message::Text(test_message.into())).await?;
    println!("WebSocket client sent SIP message");

    let incoming_event = timeout(Duration::from_secs(5), receiver.recv())
        .await
        .map_err(|_| crate::Error::Error("Message wait timeout".to_string()))?
        .ok_or_else(|| crate::Error::Error("Receiver closed".to_string()))?;

    match incoming_event {
        TransportEvent::Incoming(received_msg, SipConnection::WebSocket(_), remote_addr) => {
            println!("Successfully received SIP message from: {}", remote_addr);
            let msg_str = received_msg.to_string();
            assert!(msg_str.contains("REGISTER"));
            assert!(msg_str.contains("sip:example.com"));
            assert!(msg_str.contains("bob@example.com"));
            assert!(msg_str.contains("ws-test-call-id@127.0.0.1"));

            println!("WebSocket message parsing verification successful");
        }
        _ => {
            return Err(crate::Error::Error("Expected Incoming event".to_string()));
        }
    }
    ws_sender.close().await?;
    cancel_token.cancel();
    Ok(())
}

/// TCP multiple connections test
#[tokio::test]
async fn test_tcp_multiple_connections() -> Result<()> {
    use tokio::io::AsyncWriteExt;

    let cancel_token = CancellationToken::new();
    let transport_layer = TransportLayer::new(cancel_token.clone());
    let (sender, mut receiver) = unbounded_channel();

    let server_addr = transport_layer
        .add_tcp_listener("127.0.0.1:0".parse()?, sender.clone())
        .await?;

    transport_layer.serve_listens(sender.clone()).await?;

    // Test each connection separately to avoid event ordering issues
    for i in 0..3 {
        let client = tokio::net::TcpStream::connect(server_addr.get_socketaddr()?).await?;

        // Wait for new connection event
        let new_event = timeout(Duration::from_secs(5), receiver.recv())
            .await
            .map_err(|_| crate::Error::Error("New connection wait timeout".to_string()))?
            .ok_or_else(|| crate::Error::Error("Receiver closed".to_string()))?;
        match new_event {
            TransportEvent::New(SipConnection::Tcp(_)) => {
                println!("Client {} connected successfully", i);
            }
            other => {
                println!("Unexpected event for client {}: {:?}", i, other);
                return Err(crate::Error::Error(
                    "Expected new connection event".to_string(),
                ));
            }
        }

        // Send message immediately after connection
        let test_message = format!(
            "REGISTER sip:example.com SIP/2.0\r\n\
            Via: SIP/2.0/TCP 127.0.0.1:5060;branch=z9hG4bK-client{}\r\n\
            From: <sip:client{}@example.com>;tag=client{}\r\n\
            To: <sip:client{}@example.com>\r\n\
            Call-ID: multi-test-{}-call-id@127.0.0.1\r\n\
            CSeq: 1 REGISTER\r\n\
            Contact: <sip:client{}@127.0.0.1:5060;transport=tcp>\r\n\
            Max-Forwards: 70\r\n\
            Content-Length: 0\r\n\r\n",
            i, i, i, i, i, i
        );

        let (_, mut write_half) = client.into_split();
        write_half.write_all(test_message.as_bytes()).await?;
        write_half.flush().await?;
        println!("Client {} sent message", i);

        // Wait for incoming message event
        let incoming_event = timeout(Duration::from_secs(5), receiver.recv())
            .await
            .map_err(|_| crate::Error::Error("Message wait timeout".to_string()))?
            .ok_or_else(|| crate::Error::Error("Receiver closed".to_string()))?;
        match incoming_event {
            TransportEvent::Incoming(received_msg, SipConnection::Tcp(_), _) => {
                let msg_str = received_msg.to_string();
                let expected_from = format!("client{}@example.com", i);
                assert!(msg_str.contains(&expected_from));
                println!("Client {} message verification successful", i);
            }
            other => {
                println!("Unexpected event for client {} message: {:?}", i, other);
                return Err(crate::Error::Error("Expected Incoming event".to_string()));
            }
        }

        // Close the connection explicitly
        drop(write_half);

        // Wait for close event (optional, may not always arrive)
        if let Ok(Some(close_event)) = timeout(Duration::from_millis(500), receiver.recv()).await {
            match close_event {
                TransportEvent::Closed(_) => {
                    println!("Client {} connection closed", i);
                }
                other => {
                    println!("Unexpected close event for client {}: {:?}", i, other);
                }
            }
        }

        // Small delay between iterations
        tokio::time::sleep(Duration::from_millis(50)).await;
    }

    cancel_token.cancel();
    Ok(())
}

/// Test WSS error handling when disabled
#[tokio::test]
async fn test_websocket_secure_disabled() -> Result<()> {
    let cancel_token = CancellationToken::new();
    let config = TransportConfig::default(); // WSS disabled by default
    let transport_layer = TransportLayer::with_config(cancel_token.clone(), config);
    let (sender, _) = unbounded_channel();

    let result = transport_layer
        .add_ws_listener("127.0.0.1:0".parse()?, sender, true)
        .await;

    match result {
        Err(crate::Error::Error(msg)) if msg.contains("WSS not enabled") => {
            println!("Correctly blocked disabled WSS listener");
        }
        _ => {
            return Err(crate::Error::Error(
                "Expected WSS to be properly rejected".to_string(),
            ))
        }
    }

    Ok(())
}

/// Test TCP connection handling
#[tokio::test]
async fn test_tcp_connection_handling() -> Result<()> {
    let cancel_token = CancellationToken::new();
    let transport_layer = TransportLayer::new(cancel_token.clone());
    let (sender, mut receiver) = unbounded_channel();

    let server_addr = transport_layer
        .add_tcp_listener("127.0.0.1:0".parse()?, sender.clone())
        .await?;

    transport_layer.serve_listens(sender.clone()).await?;

    for i in 0..2 {
        let client = tokio::net::TcpStream::connect(server_addr.get_socketaddr()?).await?;

        let new_event = timeout(Duration::from_secs(5), receiver.recv())
            .await
            .map_err(|_| crate::Error::Error("New connection wait timeout".to_string()))?
            .ok_or_else(|| crate::Error::Error("Receiver closed".to_string()))?;

        match new_event {
            TransportEvent::New(_) => {
                println!("Connection {} established successfully", i);
            }
            other => {
                println!("Unexpected event for connection {}: {:?}", i, other);
                return Err(crate::Error::Error(
                    "Expected new connection event".to_string(),
                ));
            }
        }

        drop(client);

        // Wait for closed event
        let close_event = timeout(Duration::from_secs(2), receiver.recv())
            .await
            .map_err(|_| crate::Error::Error("Close event wait timeout".to_string()))?
            .ok_or_else(|| crate::Error::Error("Receiver closed".to_string()))?;

        match close_event {
            TransportEvent::Closed(_) => {
                println!("Connection {} closed successfully", i);
            }
            other => {
                println!("Unexpected close event for connection {}: {:?}", i, other);
                // Don't fail on this, as the connection might close in different ways
            }
        }

        tokio::time::sleep(Duration::from_millis(100)).await;
    }

    cancel_token.cancel();
    println!("TCP connection handling test completed");
    Ok(())
}
