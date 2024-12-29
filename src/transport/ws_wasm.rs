use std::sync::Arc;

struct WsWasmTransportInner {
}

#[derive(Clone)]
pub struct WsWasmTransport {
    inner: Arc<WsWasmTransportInner>,
}

impl WsWasmTransport {
    pub async fn send(&self, msg: rsip::SipMessage) -> crate::Result<()> {
        todo!()
    }
}

impl std::fmt::Display for WsWasmTransport {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "*:*")
    }
}
