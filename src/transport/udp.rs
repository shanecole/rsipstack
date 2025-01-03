use crate::{transport::TransportEvent, Result};
use std::{net::SocketAddr, sync::Arc};
use tokio::net::UdpSocket;
use tracing::{error, info, instrument, trace};

use super::{
    transport::{SipAddr, TransportSender},
    Transport,
};

struct UdpTransportInner {
    pub(self) conn: UdpSocket,
    pub(self) addr: SipAddr,
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

        let addr = SipAddr {
            r#type: Some(rsip::transport::Transport::Udp),
            addr: external.unwrap_or(local),
        };

        let t = UdpTransport {
            inner: Arc::new(UdpTransportInner { addr, conn }),
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
            } else if len == 1 && buf[0].is_ascii_whitespace() {
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

    #[instrument(skip(self, msg), fields(addr = %self.get_addr()))]
    pub async fn send(&self, msg: rsip::SipMessage) -> crate::Result<()> {
        let target = Transport::get_target(&msg)?;
        let buf = msg.to_string();

        trace!("sending {} -> {} {}", buf.len(), target, buf);

        self.inner
            .conn
            .send_to(buf.as_bytes(), target)
            .await
            .map_err(|e| {
                crate::Error::TransportLayerError(e.to_string(), self.get_addr().to_owned())
            })
            .map(|_| ())
    }

    pub fn get_addr(&self) -> &SipAddr {
        &self.inner.addr
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

impl std::fmt::Debug for UdpTransport {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.inner.addr)
    }
}

impl Drop for UdpTransportInner {
    fn drop(&mut self) {
        info!("dropping UDP transport: {}", self.addr);
    }
}
