use crate::{transport::TransportEvent, Result};
use std::{net::SocketAddr, sync::Arc};
use tokio::net::UdpSocket;
use tracing::{error, info, trace};

use super::{transport::TransportSender, Transport};

struct UdpTransportInner {
    pub(self) conn: UdpSocket,
    pub external: Option<SocketAddr>,
    pub local: SocketAddr,
}

#[derive(Clone)]
pub struct UdpTransport {
    inner: Arc<UdpTransportInner>,
}

impl UdpTransport {
    pub async fn create_connection(
        local: SocketAddr,
        external: Option<SocketAddr>,
    ) -> Result<Self> {
        let conn = UdpSocket::bind(local).await?;
        let t = UdpTransport {
            inner: Arc::new(UdpTransportInner {
                conn,
                external,
                local,
            }),
        };
        info!("created UDP transport: {} external: {:?}", t, external);
        Ok(t)
    }

    pub async fn serve_loop(&self, sender: TransportSender) -> Result<()> {
        let mut buf = vec![0u8; 2048];
        loop {
            let (len, addr) = match self.inner.conn.recv_from(&mut buf).await {
                Ok((len, addr)) => (len, addr),
                Err(e) => {
                    error!("Error receiving UDP packet: {}", e);
                    continue;
                }
            };

            // '\r\n' is the keepalive message
            if len == 2 && buf[0] == 13 && buf[1] == 10 {
                // '\r\n'
                continue;
            }

            let undecoded = match std::str::from_utf8(&buf[..len]) {
                Ok(s) => s,
                Err(e) => {
                    info!(
                        "Error decoding SIP message from: {} error: {} buf: {:?}",
                        addr,
                        e,
                        &buf[..len]
                    );
                    continue;
                }
            };

            let msg = match rsip::SipMessage::try_from(undecoded) {
                Ok(msg) => msg,
                Err(e) => {
                    info!(
                        "Error parsing SIP message from: {} error: {} buf: {}",
                        addr, e, undecoded
                    );
                    continue;
                }
            };

            trace!(
                "received {} {} -> {} {}",
                len,
                addr,
                self.get_addr(),
                undecoded
            );

            let event = TransportEvent::IncomingMessage(msg, Transport::Udp(self.clone()));
            sender.send(event)?;
        }
    }

    pub async fn send(&self, msg: rsip::SipMessage) -> crate::Result<()> {
        let target = Transport::get_target(&msg)?;
        let buf = msg.to_string();

        trace!(
            "sending {} {} -> {} {}",
            buf.len(),
            self.get_addr(),
            target,
            buf
        );

        self.inner
            .conn
            .send_to(buf.as_bytes(), target)
            .await
            .map_err(|e| {
                crate::Error::TransportLayerError(e.to_string(), self.get_addr().to_owned())
            })
            .map(|_| ())
    }

    pub fn get_addr(&self) -> &std::net::SocketAddr {
        self.inner.external.as_ref().unwrap_or(&self.inner.local)
    }
}

impl std::fmt::Display for UdpTransport {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self.inner.conn.local_addr() {
            Ok(addr) => write!(f, "{}", addr),
            Err(_) => write!(f, "*:*"),
        }
    }
}

impl Drop for UdpTransport {
    fn drop(&mut self) {
        info!("dropping UDP transport: {}", self);
    }
}
