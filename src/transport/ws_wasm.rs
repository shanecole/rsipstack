use super::connection::{SipAddr, TransportSender};
use crate::Result;
use std::sync::Arc;

struct WsWasmTransportInner {}

#[derive(Clone)]
pub struct WsWasmConnection {
    inner: Arc<WsWasmTransportInner>,
}

impl WsWasmConnection {
    pub async fn send(&self, msg: rsip::SipMessage) -> crate::Result<()> {
        todo!()
    }
    pub fn get_addr(&self) -> &SipAddr {
        todo!()
    }

    pub async fn serve_loop(&self, sender: TransportSender) -> Result<()> {
        todo!()
    }
}

impl std::fmt::Display for WsWasmConnection {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "*:*")
    }
}
impl std::fmt::Debug for WsWasmConnection {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "*:*")
    }
}
