use crate::{
    transport::{
        connection::TransportEvent, stream::StreamConnection, tcp::TcpConnection,
        transport_layer::TransportConfig, TransportLayer,
    },
    Result,
};
use rsip::SipMessage;
use std::time::Duration;
use tokio::{
    sync::mpsc::{self, UnboundedReceiver},
    time::timeout,
};
use tokio_util::sync::CancellationToken;
use tracing::info;

/// Test TCP connection lifecycle management
#[tokio::test]
async fn test_tcp_connection_lifecycle() -> Result<()> {
    let cancel_token = CancellationToken::new();
    let transport_layer = TransportLayer::new(cancel_token.clone());
    let (sender, mut receiver) = mpsc::unbounded_channel();

    // Create TCP listener
    let server_addr = transport_layer
        .add_tcp_listener("127.0.0.1:0".parse()?, sender.clone())
        .await?;
    info!("Created TCP server on {}", server_addr);

    // Start transport layer
    transport_layer.serve_listens(sender.clone()).await?;

    // Create client connection
    let client = TcpConnection::connect(&server_addr).await?;
    info!("Created TCP client connection: {}", client);

    // Wait for new connection event
    let new_event = wait_for_event(&mut receiver).await?;
    match new_event {
        TransportEvent::New(conn) => {
            info!("New connection established: {}", conn.get_addr());
        }
        _ => panic!("Expected new connection event"),
    }

    // Send a test message
    let test_message = create_test_message("test-tcp-lifecycle");
    client.send_message(test_message.clone()).await?;

    // Wait for incoming message
    let incoming_event = wait_for_event(&mut receiver).await?;
    match incoming_event {
        TransportEvent::Incoming(msg, _conn, _addr) => {
            info!("Received message: {}", msg);
        }
        _ => panic!("Expected incoming message event"),
    }

    // Close the client connection
    client.close().await?;

    // Wait for connection closed event
    let closed_event = wait_for_event(&mut receiver).await?;
    match closed_event {
        TransportEvent::Closed(conn) => {
            info!("Connection closed: {}", conn.get_addr());
        }
        _ => panic!("Expected connection closed event"),
    }

    cancel_token.cancel();
    Ok(())
}

/// Test WebSocket connection lifecycle management
#[tokio::test]
async fn test_websocket_connection_lifecycle() -> Result<()> {
    let cancel_token = CancellationToken::new();
    let mut config = TransportConfig::default();
    config.enable_ws = true;

    let transport_layer = TransportLayer::with_config(cancel_token.clone(), config);
    let (sender, _receiver) = mpsc::unbounded_channel();

    // Create WebSocket listener
    let server_addr = transport_layer
        .add_ws_listener("127.0.0.1:0".parse()?, sender.clone(), false)
        .await?;
    info!("Created WebSocket server on {}", server_addr);

    // Start transport layer
    transport_layer.serve_listens(sender.clone()).await?;

    // Note: For full WebSocket testing, we would need a WebSocket client
    // This test demonstrates the setup for lifecycle management

    cancel_token.cancel();
    Ok(())
}

/// Test multiple connections and their lifecycle
#[tokio::test]
async fn test_multiple_connection_lifecycle() -> Result<()> {
    let cancel_token = CancellationToken::new();
    let transport_layer = TransportLayer::new(cancel_token.clone());
    let (sender, mut receiver) = mpsc::unbounded_channel();

    // Create TCP listener
    let server_addr = transport_layer
        .add_tcp_listener("127.0.0.1:0".parse()?, sender.clone())
        .await?;

    transport_layer.serve_listens(sender.clone()).await?;

    // Create multiple client connections
    let client1 = TcpConnection::connect(&server_addr).await?;
    let client2 = TcpConnection::connect(&server_addr).await?;
    let client3 = TcpConnection::connect(&server_addr).await?;

    // Wait for new connection events
    let mut new_connections = 0;
    for _ in 0..3 {
        match wait_for_event(&mut receiver).await? {
            TransportEvent::New(_conn) => {
                new_connections += 1;
                info!("New connection #{}", new_connections);
            }
            event => info!("Other event: {:?}", event),
        }
    }
    assert_eq!(new_connections, 3);

    // Close connections in sequence
    client1.close().await?;
    client2.close().await?;
    client3.close().await?;

    // Wait for closed connection events
    let mut closed_connections = 0;
    for _ in 0..3 {
        match wait_for_event(&mut receiver).await? {
            TransportEvent::Closed(_conn) => {
                closed_connections += 1;
                info!("Closed connection #{}", closed_connections);
            }
            event => info!("Other event: {:?}", event),
        }
    }
    assert_eq!(closed_connections, 3);

    cancel_token.cancel();
    Ok(())
}

/// Test connection error handling and cleanup
#[tokio::test]
async fn test_connection_error_cleanup() -> Result<()> {
    let cancel_token = CancellationToken::new();
    let transport_layer = TransportLayer::new(cancel_token.clone());
    let (sender, mut receiver) = mpsc::unbounded_channel();

    // Create TCP listener
    let server_addr = transport_layer
        .add_tcp_listener("127.0.0.1:0".parse()?, sender.clone())
        .await?;

    transport_layer.serve_listens(sender.clone()).await?;

    // Create client connection
    let client = TcpConnection::connect(&server_addr).await?;

    // Wait for new connection event
    wait_for_event(&mut receiver).await?;

    // Force close by dropping the client
    drop(client);

    // Should eventually receive a closed event
    tokio::time::sleep(Duration::from_millis(100)).await;

    cancel_token.cancel();
    Ok(())
}

/// Helper function to create test SIP message
fn create_test_message(call_id: &str) -> SipMessage {
    let message = format!(
        "REGISTER sip:example.com SIP/2.0\r\n\
         Via: SIP/2.0/TCP 127.0.0.1:5060;branch=z9hG4bK-{}\r\n\
         From: <sip:alice@example.com>;tag={}\r\n\
         To: <sip:alice@example.com>\r\n\
         Call-ID: {}\r\n\
         CSeq: 1 REGISTER\r\n\
         Contact: <sip:alice@127.0.0.1:5060>\r\n\
         Max-Forwards: 70\r\n\
         Content-Length: 0\r\n\r\n",
        call_id, call_id, call_id
    );
    SipMessage::try_from(message.as_str()).expect("create test message")
}

/// Helper function to wait for transport events with timeout
async fn wait_for_event(
    receiver: &mut UnboundedReceiver<TransportEvent>,
) -> Result<TransportEvent> {
    match timeout(Duration::from_secs(5), receiver.recv()).await {
        Ok(Some(event)) => Ok(event),
        Ok(None) => Err(crate::Error::Error("Channel closed".to_string())),
        Err(_) => Err(crate::Error::Error("Timeout waiting for event".to_string())),
    }
}
