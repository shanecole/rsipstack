use std::sync::Arc;

struct UdpTransportInner {
    pub(self) conn: tokio::net::UdpSocket,
}

#[derive(Clone)]
pub struct UdpTransport {
    inner: Arc<UdpTransportInner>,
}

impl UdpTransport {
    pub async fn send(&self, msg: rsip::SipMessage) -> crate::Result<()> {
        todo!()
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
