# rsipstack - A SIP Stack written in Rust

**WIP** This is a work in progress and is not yet ready for production use.

A RFC 3261 compliant SIP stack written in Rust. The goal of this project is to provide a high-performance, reliable, and easy-to-use SIP stack that can be used in various scenarios.

## Features

- **RFC 3261 Compliant**: Full compliance with SIP specification
- **Multiple Transport Support**: UDP, TCP, TLS, WebSocket
- **Transaction Layer**: Complete SIP transaction state machine
- **Dialog Layer**: SIP dialog management
- **Digest Authentication**: Built-in authentication support
- **High Performance**: Built with Rust for maximum performance
- **Easy to Use**: Simple and intuitive API design

## TODO
- [x] Transport support
  - [x] UDP
  - [x] TCP
  - [x] TLS
  - [x] WebSocket
- [x] Digest Authentication
- [x] Transaction Layer
- [x] Dialog Layer
- [ ] WASM target

## Use Cases

This SIP stack can be used in various scenarios, including but not limited to:

- Integration with WebRTC for browser-based communication, such as WebRTC SBC.
- Building custom SIP proxies or registrars
- Building custom SIP user agents (SIP.js alternative)

## Why Rust?

We are a group of developers who are passionate about SIP and Rust. We believe that Rust is a great language for building high-performance network applications, and we want to bring the power of Rust to the SIP/WebRTC/SFU world.

## Quick Start Examples

### 1. Simple SIP Connection

The most basic way to use rsipstack is through direct SIP connections:

```bash
cargo run --example simple_connection
```

This example demonstrates:
- Creating UDP/TCP connections
- Sending raw SIP messages
- Basic message handling

### 2. Endpoint, TransportLayer and Transaction

For more advanced usage with transaction handling:

```bash
cargo run --example endpoint_transaction
```

This example shows:
- Creating and configuring an Endpoint
- Setting up TransportLayer with multiple transports
- Handling SIP transactions (client and server)
- Processing various SIP methods (OPTIONS, REGISTER, MESSAGE)

### 3. SIP User Agent (Register/Invite)

A complete SIP user agent implementation:

```bash
cargo run --example ua_register_invite
```

Features:
- User registration with SIP registrar
- Making and receiving INVITE calls
- Call establishment and termination
- Dialog management
- Authentication handling

### 4. SIP Proxy (Register/Invite)

A SIP proxy server implementation:

```bash
cargo run --example proxy_register_invite
```

Features:
- SIP message routing
- Registration handling
- Call forwarding
- Multiple transport support
- Load balancing

## API Usage Guide

### 1. Simple SIP Connection

```rust
use rsipstack::transport::{udp::UdpConnection, SipAddr};

// Create UDP connection
let connection = UdpConnection::create_connection("127.0.0.1:5060".parse()?, None).await?;

// Send raw SIP message
let sip_message = "MESSAGE sip:test@example.com SIP/2.0\r\n...";
connection.send_raw(sip_message.as_bytes(), &target_addr).await?;
```

### 2. Using Endpoint and Transactions

```rust
use rsipstack::{EndpointBuilder, transaction::TransactionType};
use rsipstack::transport::TransportLayer;

// Build endpoint
let transport_layer = TransportLayer::new(cancel_token);
let endpoint = EndpointBuilder::new()
    .with_transport_layer(transport_layer)
    .build();

// Handle incoming transactions
let mut incoming = endpoint.incoming_transactions();
while let Some(transaction) = incoming.recv().await {
    // Process transaction based on method
    match transaction.original.method {
        rsip::Method::Register => {
            transaction.respond(rsip::StatusCode::OK, None, None).await?;
        }
        // ... handle other methods
    }
}
```

### 3. Creating a User Agent

```rust
use rsipstack::dialog::{DialogLayer, InviteDialog};

// Create dialog layer
let dialog_layer = DialogLayer::new(endpoint);

// Register with server
let register_dialog = dialog_layer.create_register_dialog(
    "sip:alice@example.com",
    "sip:registrar.example.com"
).await?;

// Make outgoing call
let invite_dialog = dialog_layer.create_invite_dialog(
    "sip:alice@example.com",
    "sip:bob@example.com"
).await?;
```

### 4. Implementing a Proxy

```rust
use rsipstack::transaction::Endpoint;

// Create proxy endpoint
let proxy = EndpointBuilder::new()
    .with_transport_layer(transport_layer)
    .build();

// Handle incoming requests
let mut incoming = proxy.incoming_transactions();
while let Some(transaction) = incoming.recv().await {
    // Route request based on URI
    let target = route_request(&transaction.original)?;
    
    // Forward request
    proxy.forward_request(transaction, target).await?;
}
```

## Running Tests

### Unit Tests
```bash
cargo test
```

### RFC 3261 Compliance Tests
```bash
# Run all compliance tests
cargo test rfc3261_compliance

# Run specific compliance test suites
cargo test rfc3261_transactions
cargo test rfc3261_dialogs
cargo test rfc3261_transport
```

### Integration Tests
```bash
# Test with real SIP scenarios
cargo test --test integration_tests
```

### Benchmark Tests
```bash
# Run server
cargo run -r --bin bench_ua  -- -m server -p 5060

# Run client with 1000 calls
cargo run -r  --bin bench_ua  -- -m client -p 5061 -s 127.0.0.1:5060 -c 1000
```

The test monitor:

```bash
=== SIP Benchmark UA Stats ===
Dialogs: 9992
Active Calls: 9983
Rejected Calls: 0
Failed Calls: 0
Total Calls: 250276
Calls/Second: 1501
============================
```

## Documentation

- [API Documentation](https://docs.rs/rsipstack)
- [Examples](./examples/)
- [RFC 3261 Compliance Tests](./tests/)

## Contributing

We welcome contributions! Please see our [Contributing Guide](CONTRIBUTING.md) for details.

## License

This project is licensed under the MIT License - see the [LICENSE](LICENSE) file for details.