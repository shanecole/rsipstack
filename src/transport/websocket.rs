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
use std::{fmt, sync::Arc};
use tokio::{net::TcpListener, sync::Mutex};
use tokio_tungstenite::{
    connect_async, tungstenite::protocol::Message, MaybeTlsStream, WebSocketStream,
};
use tracing::{debug, error, info, warn};

// Define a type alias for the WebSocket sink to make the code more readable
type WsSink = futures_util::stream::SplitSink<
    WebSocketStream<MaybeTlsStream<tokio::net::TcpStream>>,
    Message,
>;
type WsRead = futures_util::stream::SplitStream<WebSocketStream<MaybeTlsStream<tokio::net::TcpStream>>>;

pub struct WebSocketInner {
    pub local_addr: SipAddr,
    pub remote_addr: Option<SipAddr>,
    pub ws_sink: Arc<Mutex<WsSink>>,
    pub ws_read: Arc<Mutex<WsRead>>,
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
        let url = format!("{}://{}:{}/sip", scheme, host, port);

        let (ws_stream, _) = connect_async(&url).await?;
        let (ws_sink, _ws_stream) = ws_stream.split();

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
                remote_addr: Some(remote.clone()),
                ws_sink: Arc::new(Mutex::new(ws_sink)),
                ws_read: Arc::new(Mutex::new(_ws_stream)),
            }),
        };

        info!(
            "Created WebSocket client connection: {} -> {}",
            connection.get_addr(),
            remote
        );

        Ok(connection)
    }

    pub async fn serve_listener(
        tcp_listener: TcpListener,
        local_addr: SipAddr,
        sender: TransportSender,
        is_secure: bool,
    ) -> Result<()> {
        let transport_type = if is_secure {
            rsip::transport::Transport::Wss
        } else {
            rsip::transport::Transport::Ws
        };

        info!("Starting WebSocket listener on {}", local_addr);

        loop {
            match tcp_listener.accept().await {
                Ok((stream, remote_addr)) => {
                    debug!("New WebSocket connection from {}", remote_addr);

                    let remote_sip_addr = SipAddr {
                        r#type: Some(transport_type.clone()),
                        addr: remote_addr.into(),
                    };

                    let local_addr_clone = local_addr.clone();
                    let sender_clone = sender.clone();

                    tokio::spawn(async move {
                        // Wrap the TCP stream in MaybeTlsStream
                        let maybe_tls_stream = MaybeTlsStream::Plain(stream);

                        // Accept the WebSocket connection
                        let ws_stream =
                            match tokio_tungstenite::accept_async(maybe_tls_stream).await {
                                Ok(ws) => ws,
                                Err(e) => {
                                    error!("Error upgrading to WebSocket: {}", e);
                                    return;
                                }
                            };

                        // Split the WebSocket stream
                        let (ws_sink, mut ws_stream) = ws_stream.split();

                        // Create a new WebSocket connection
                        let connection = WebSocketConnection {
                            inner: Arc::new(WebSocketInner {
                                local_addr: local_addr_clone.clone(),
                                remote_addr: Some(remote_sip_addr.clone()),
                                ws_sink: Arc::new(Mutex::new(ws_sink)),
                                ws_read: Arc::new(Mutex::new(ws_stream)),
                            }),
                        };
                        let sip_connection = SipConnection::WebSocket(connection.clone());

                        if let Err(e) =
                            sender_clone.send(TransportEvent::New(sip_connection.clone()))
                        {
                            error!("Error sending new connection event: {:?}", e);
                            return;
                        }
                        connection.serve_loop(sender_clone.clone()).await;
                    });
                }
                Err(e) => {
                    error!("Error accepting TCP connection for WebSocket: {}", e);
                }
            }
        }
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
        info!("WebSocket send:{}",data);
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
        let remote_addr = self.inner.remote_addr.clone().unwrap().clone();
        let mut ws_read = self.inner.ws_read.lock().await;
        while let Some(msg) = ws_read.next().await {
            match msg {
                Ok(Message::Text(text)) => {
                    match SipMessage::try_from(text.as_str()) {
                        Ok(sip_msg) => {
                            if let Err(e) =
                                sender.send(TransportEvent::Incoming(
                                    sip_msg,
                                    sip_connection.clone(),
                                    remote_addr.clone(),
                                ))
                            {
                                error!("Error sending incoming message: {:?}", e);
                                break;
                            }
                        }
                        Err(e) => {
                            warn!("Error parsing SIP message: {}", e);
                        }
                    }
                }
                Ok(Message::Binary(bin)) => {
                    if bin == KEEPALIVE_REQUEST {
                        if let Err(e) =
                            self.send_raw(KEEPALIVE_RESPONSE).await
                        {
                            error!("Error sending keepalive response: {:?}", e);
                        }
                        continue;
                    }

                    match std::str::from_utf8(&bin) {
                        Ok(text) => match SipMessage::try_from(text) {
                            Ok(sip_msg) => {
                                if let Err(e) =
                                    sender.send(TransportEvent::Incoming(
                                        sip_msg,
                                        sip_connection.clone(),
                                        remote_addr.clone(),
                                    ))
                                {
                                    error!(
                                        "Error sending incoming message: {:?}",
                                        e
                                    );
                                    break;
                                }
                            }
                            Err(e) => {
                                warn!("Error parsing SIP message: {}", e);
                            }
                        },
                        Err(e) => {
                            warn!("Error decoding binary message: {}", e);
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

        if let Err(e) = sender.send(TransportEvent::Closed(sip_connection.clone())) {
            error!("Error sending connection closed event: {:?}", e);
        }
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
