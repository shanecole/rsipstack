use crate::{
    transport::{
        connection::{SipAddr, TransportSender, KEEPALIVE_REQUEST, KEEPALIVE_RESPONSE},
        SipConnection, TransportEvent
    },
    Result,
};
use std::net::SocketAddr;
use std::sync::Arc;
use futures::{
    stream::{SplitSink, SplitStream},
    SinkExt, StreamExt
};
use tokio::{
    io::AsyncReadExt,
    net::TcpStream,
    sync::{RwLock,RwLockReadGuard}
};
use tracing::{info, debug};
use tokio_tungstenite::{
    connect_async,
    tungstenite::protocol::Message, MaybeTlsStream, WebSocketStream
};
use crate::transport::udp::UdpInner;

struct WsInner {
    pub write: RwLock<SplitSink<WebSocketStream<MaybeTlsStream<TcpStream>>, Message>>,
    pub read: RwLock<SplitStream<WebSocketStream<MaybeTlsStream<TcpStream>>>>,
    pub addr: SipAddr,
}

#[derive(Clone)]
pub struct WsConnection {
    pub external: Option<SipAddr>,
    inner: Arc<WsInner>,
}

impl WsConnection {
    pub async fn create_connection(
        local: SocketAddr,
        external: Option<SocketAddr>,
    ) -> Result<Self> {
        match connect_async(format!("ws://{}:{}", local.ip(), local.port())).await {
            Ok((conn, _)) => {
                let addr = SipAddr {
                    r#type: Some(rsip::transport::Transport::Ws),
                    addr: local.into(),
                };
                let (mut write, mut read) = conn.split();
                let t = WsConnection {
                    external: external.map(|addr| SipAddr {
                        r#type: Some(rsip::transport::Transport::Ws),
                        addr: addr.into(),
                    }),
                    inner: Arc::new(WsInner { addr,  write: RwLock::new(write), read: RwLock::new(read) }),
                };
                info!("created Ws connection: {} external: {:?}", t, external);
                Ok(t)
            }
            Err(_) => {
                Err(crate::Error::Error(
                    "WsConnection failed to connect".to_string(),
                ))
            }
        }
    }

    pub async fn send(&self, msg: rsip::SipMessage) -> crate::Result<()> {
        let buf = msg.to_string();
        debug!("send {} -> {}", buf.len(), buf);
        let mut write = self.inner.write.write().await;
        write.send(Message::Text(buf))
            .await
            .map_err(|e| {
                crate::Error::TransportLayerError(e.to_string(), self.get_addr().to_owned())
            })
            .map(|_| ())
    }
    pub fn get_addr(&self) -> &SipAddr {
        if let Some(external) = &self.external {
            external
        } else {
            &self.inner.addr
        }
    }

    pub async fn serve_loop(&self, sender: TransportSender) -> crate::Result<()> {
        let mut read = self.inner.read.write().await;
        loop {
            if let Some(msg) = read.next().await {
                match msg {
                    Ok(Message::Text(t)) => {

                        match t.as_bytes() {
                            KEEPALIVE_REQUEST => {
                                let _ = self.inner.write.write().await.send(Message::Text(String::from_utf8(KEEPALIVE_RESPONSE.to_vec()).unwrap()));
                                continue;
                            }
                            KEEPALIVE_RESPONSE => continue,
                            _ => {
                                if t.as_bytes().iter().all(|&b| b.is_ascii_whitespace()) {
                                    continue;
                                }
                            }
                        }

                        debug!("Received: {:?}", t);

                        let msg = match rsip::SipMessage::try_from(t) {
                            Ok(msg) => msg,
                            Err(e) => {
                                info!(
                                    "error parsing SIP message error: {} ",
                                    e
                                );
                                continue;
                            }
                        };

                        sender.send(TransportEvent::Incoming(
                            msg,
                            SipConnection::Ws(self.clone()),
                            SipAddr {
                                r#type: Some(rsip::transport::Transport::Ws),
                                addr: self.inner.addr.addr.clone(),
                            },
                        ))?;
                    },
                    Ok(Message::Binary(b)) => {
                        todo!()
                    },
                    Ok(Message::Close(_)) => {

                    },
                    Ok(Message::Ping(_)) => {

                    },
                    Ok(Message::Pong(_)) => {

                    },
                    Ok(Message::Frame(_)) => {
                        todo!();
                    },
                    Err(e) => {
                        continue;
                    }
                }
            }
        }
    }

}

impl std::fmt::Display for WsConnection {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "*:*")
    }
}
impl std::fmt::Debug for WsConnection {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "*:*")
    }
}

impl Drop for WsInner {
    fn drop(&mut self) {
        info!("dropping WS transport: {}", self.addr);
    }
}