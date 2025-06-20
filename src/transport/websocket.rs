use crate::{
    transport::{
        connection::{TransportSender, KEEPALIVE_REQUEST, KEEPALIVE_RESPONSE},
        sip_addr::SipAddr,
        stream::StreamConnection,
        SipConnection, TransportEvent,
    },
    Result,
};
use futures_util::{SinkExt, StreamExt};
use rsip::SipMessage;
use std::{fmt, net::SocketAddr, sync::Arc};
use tokio::{net::TcpListener, select, sync::Mutex};
use tokio_tungstenite::{
    connect_async,
    tungstenite::{
        client::IntoClientRequest,
        handshake::server::{Request, Response},
        protocol::Message,
    },
    MaybeTlsStream, WebSocketStream,
};
use tokio_util::sync::CancellationToken;
use tracing::{debug, error, info, warn};

// Define a type alias for the WebSocket sink to make the code more readable
type WsSink = futures_util::stream::SplitSink<
    WebSocketStream<MaybeTlsStream<tokio::net::TcpStream>>,
    Message,
>;
type WsRead =
    futures_util::stream::SplitStream<WebSocketStream<MaybeTlsStream<tokio::net::TcpStream>>>;

// WebSocket Listener Connection Structure
pub struct WebSocketListenerConnectionInner {
    pub local_addr: SipAddr,
    pub external: Option<SipAddr>,
    pub is_secure: bool,
}

#[derive(Clone)]
pub struct WebSocketListenerConnection {
    pub inner: Arc<WebSocketListenerConnectionInner>,
}

impl WebSocketListenerConnection {
    pub async fn new(
        local_addr: SipAddr,
        external: Option<SocketAddr>,
        is_secure: bool,
    ) -> Result<Self> {
        let transport_type = if is_secure {
            rsip::transport::Transport::Wss
        } else {
            rsip::transport::Transport::Ws
        };

        let inner = WebSocketListenerConnectionInner {
            local_addr,
            external: external.map(|addr| SipAddr {
                r#type: Some(transport_type.clone()),
                addr: addr.into(),
            }),
            is_secure,
        };
        Ok(WebSocketListenerConnection {
            inner: Arc::new(inner),
        })
    }

    pub async fn serve_listener(
        &self,
        cancel_token: CancellationToken,
        sender: TransportSender,
    ) -> Result<()> {
        let listener = TcpListener::bind(self.inner.local_addr.get_socketaddr()?).await?;
        let transport_type = if self.inner.is_secure {
            rsip::transport::Transport::Wss
        } else {
            rsip::transport::Transport::Ws
        };
        let sender = sender.clone();
        let cancel_token = cancel_token.clone();

        info!("Starting WebSocket listener on {}", self.inner.local_addr);

        tokio::spawn(async move {
            loop {
                let (stream, remote_addr) = match listener.accept().await {
                    Ok((stream, remote_addr)) => (stream, remote_addr),
                    Err(e) => {
                        warn!("Failed to accept WebSocket connection: {:?}", e);
                        continue;
                    }
                };

                debug!("New WebSocket connection from {}", remote_addr);

                let remote_sip_addr = SipAddr {
                    r#type: Some(transport_type.clone()),
                    addr: remote_addr.into(),
                };

                let sender_clone = sender.clone();
                let cancel_token = cancel_token.child_token();

                tokio::spawn(async move {
                    // Wrap the TCP stream in MaybeTlsStream
                    let maybe_tls_stream = MaybeTlsStream::Plain(stream);

                    // Accept the WebSocket connection with custom header handling
                    let callback = |req: &Request, mut response: Response| {
                        // Check if client requested 'sip' subprotocol
                        if let Some(protocols) = req.headers().get("sec-websocket-protocol") {
                            if let Ok(protocols_str) = protocols.to_str() {
                                if protocols_str.contains("sip") {
                                    // Add the 'sip' subprotocol to response
                                    response
                                        .headers_mut()
                                        .insert("sec-websocket-protocol", "sip".parse().unwrap());
                                }
                            }
                        }
                        Ok(response)
                    };

                    let ws_stream =
                        match tokio_tungstenite::accept_hdr_async(maybe_tls_stream, callback).await
                        {
                            Ok(ws) => ws,
                            Err(e) => {
                                error!("Error upgrading to WebSocket: {}", e);
                                return;
                            }
                        };

                    let (ws_sink, ws_read) = ws_stream.split();
                    let local_addr = SipAddr {
                        r#type: Some(transport_type.clone()),
                        addr: remote_addr.into(),
                    };

                    Self::serve_ws_connection(
                        cancel_token,
                        ws_sink,
                        ws_read,
                        local_addr,
                        remote_sip_addr,
                        sender_clone,
                    )
                    .await;
                });
            }
        });
        Ok(())
    }

    pub fn get_addr(&self) -> &SipAddr {
        if let Some(external) = &self.inner.external {
            external
        } else {
            &self.inner.local_addr
        }
    }

    pub async fn close(&self) -> Result<()> {
        Ok(())
    }

    async fn serve_ws_connection(
        cancel_token: CancellationToken,
        ws_sink: WsSink,
        ws_read: WsRead,
        local_addr: SipAddr,
        remote_addr: SipAddr,
        sender: TransportSender,
    ) {
        // Create a new WebSocket connection
        let connection = WebSocketConnection {
            inner: Arc::new(WebSocketInner {
                local_addr,
                remote_addr,
                ws_sink: Mutex::new(ws_sink),
                ws_read: Mutex::new(Some(ws_read)),
            }),
        };
        let sip_connection = SipConnection::WebSocket(connection.clone());
        let connection_addr = connection.get_addr().clone();

        info!("Added WebSocket connection: {}", connection_addr);

        if let Err(e) = sender.send(TransportEvent::New(sip_connection.clone())) {
            error!("Error sending new connection event: {:?}", e);
            return;
        }

        select! {
            _ = cancel_token.cancelled() => {
            }
            _ = connection.serve_loop(sender.clone()) => {
                info!(
                    "WebSocket connection serve loop completed: {}",
                    connection_addr
                );
            }
        }

        info!("Removed WebSocket connection: {}", connection_addr);
        if let Err(e) = sender.send(TransportEvent::Closed(sip_connection)) {
            warn!("Error sending WebSocket connection closed event: {:?}", e);
        }
    }
}

impl fmt::Display for WebSocketListenerConnection {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let transport = if self.inner.is_secure { "WSS" } else { "WS" };
        write!(f, "{} Listener {}", transport, self.get_addr())
    }
}

impl fmt::Debug for WebSocketListenerConnection {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        fmt::Display::fmt(self, f)
    }
}

pub struct WebSocketInner {
    pub local_addr: SipAddr,
    pub remote_addr: SipAddr,
    pub ws_sink: Mutex<WsSink>,
    pub ws_read: Mutex<Option<WsRead>>,
}

#[derive(Clone)]
pub struct WebSocketConnection {
    pub inner: Arc<WebSocketInner>,
}

impl WebSocketConnection {
    pub async fn connect(remote: &SipAddr) -> Result<Self> {
        let scheme = match remote.r#type {
            Some(rsip::transport::Transport::Wss) => "wss",
            _ => "ws",
        };

        let host = match &remote.addr.host {
            rsip::host_with_port::Host::Domain(domain) => domain.to_string(),
            rsip::host_with_port::Host::IpAddr(ip) => ip.to_string(),
        };

        let port = remote.addr.port.as_ref().map_or(5060, |p| *p.value());

        let url = format!("{}://{}:{}/", scheme, host, port);
        let mut request = url.into_client_request()?;
        request
            .headers_mut()
            .insert("sec-websocket-protocol", "sip".parse().unwrap());

        let (ws_stream, _) = connect_async(request).await?;
        let (ws_sink, ws_stream) = ws_stream.split();

        let local_addr = SipAddr {
            r#type: Some(if scheme == "wss" {
                rsip::transport::Transport::Wss
            } else {
                rsip::transport::Transport::Ws
            }),
            addr: remote.addr.clone(),
        };

        let connection = WebSocketConnection {
            inner: Arc::new(WebSocketInner {
                local_addr,
                remote_addr: remote.clone(),
                ws_sink: Mutex::new(ws_sink),
                ws_read: Mutex::new(Some(ws_stream)),
            }),
        };

        info!(
            "Created WebSocket client connection: {} -> {}",
            connection.get_addr(),
            remote
        );

        Ok(connection)
    }
}

#[async_trait::async_trait]
impl StreamConnection for WebSocketConnection {
    fn get_addr(&self) -> &SipAddr {
        &self.inner.local_addr
    }

    async fn send_message(&self, msg: SipMessage) -> Result<()> {
        let data = msg.to_string();
        let mut sink = self.inner.ws_sink.lock().await;
        info!("WebSocket send:{}", data);
        sink.send(Message::Text(data.into())).await?;
        Ok(())
    }

    async fn send_raw(&self, data: &[u8]) -> Result<()> {
        let mut sink = self.inner.ws_sink.lock().await;
        sink.send(Message::Binary(data.to_vec().into())).await?;
        Ok(())
    }

    async fn serve_loop(&self, sender: TransportSender) -> Result<()> {
        let sip_connection = SipConnection::WebSocket(self.clone());

        let remote_addr = self.inner.remote_addr.clone();
        let mut ws_read = match self.inner.ws_read.lock().await.take() {
            Some(ws_read) => ws_read,
            None => {
                error!("WebSocket connection closed");
                return Ok(());
            }
        };
        while let Some(msg) = ws_read.next().await {
            debug!(?remote_addr, "WebSocket message: {:?}", msg);
            match msg {
                Ok(Message::Text(text)) => match SipMessage::try_from(text.as_str()) {
                    Ok(sip_msg) => {
                        if let Err(e) = sender.send(TransportEvent::Incoming(
                            sip_msg,
                            sip_connection.clone(),
                            remote_addr.clone(),
                        )) {
                            error!("Error sending incoming message: {:?}", e);
                            break;
                        }
                    }
                    Err(e) => {
                        warn!("Error parsing SIP message: {}", e);
                    }
                },
                Ok(Message::Binary(bin)) => {
                    if bin == *KEEPALIVE_REQUEST {
                        if let Err(e) = self.send_raw(KEEPALIVE_RESPONSE).await {
                            error!("Error sending keepalive response: {:?}", e);
                        }
                        continue;
                    }
                    match SipMessage::try_from(bin) {
                        Ok(sip_msg) => {
                            if let Err(e) = sender.send(TransportEvent::Incoming(
                                sip_msg,
                                sip_connection.clone(),
                                remote_addr.clone(),
                            )) {
                                error!("Error sending incoming message: {:?}", e);
                                break;
                            }
                        }
                        Err(e) => {
                            warn!("Error parsing SIP message: {}", e);
                        }
                    }
                }
                Ok(Message::Ping(data)) => {
                    let mut sink = self.inner.ws_sink.lock().await;
                    if let Err(e) = sink.send(Message::Pong(data)).await {
                        error!("Error sending pong: {}", e);
                        break;
                    }
                }
                Ok(Message::Close(_)) => {
                    debug!("WebSocket connection closed by peer");
                    break;
                }
                Err(e) => {
                    error!("WebSocket error: {}", e);
                    break;
                }
                _ => {}
            }
        }

        // Note: Connection closed event is now handled in serve_listener
        // to avoid duplicate events
        debug!("WebSocket serve_loop exiting: {}", remote_addr);
        Ok(())
    }

    async fn close(&self) -> Result<()> {
        let mut sink = self.inner.ws_sink.lock().await;
        sink.send(Message::Close(None)).await?;
        Ok(())
    }
}

impl fmt::Display for WebSocketConnection {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let transport = match self.inner.local_addr.r#type {
            Some(rsip::transport::Transport::Wss) => "WSS",
            _ => "WS",
        };
        write!(f, "{} {}", transport, self.inner.local_addr)
    }
}

impl fmt::Debug for WebSocketConnection {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        fmt::Display::fmt(self, f)
    }
}
