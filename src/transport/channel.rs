use super::{
    connection::{SipAddr, TransportReceiver, TransportSender},
    SipConnection,
};
use crate::Result;
use std::sync::{Arc, Mutex};

struct ChannelInner {
    incoming: Mutex<Option<TransportReceiver>>,
    outgoing: TransportSender,
    addr: SipAddr,
}

#[derive(Clone)]
pub struct ChannelConnection {
    inner: Arc<ChannelInner>,
}

impl ChannelConnection {
    pub async fn create_connection(
        incoming: TransportReceiver,
        outgoing: TransportSender,
        addr: SipAddr,
    ) -> Result<Self> {
        let t = ChannelConnection {
            inner: Arc::new(ChannelInner {
                incoming: Mutex::new(Some(incoming)),
                outgoing,
                addr,
            }),
        };
        Ok(t)
    }

    pub async fn send(&self, msg: rsip::SipMessage) -> crate::Result<()> {
        let transport = SipConnection::Channel(self.clone());
        let source = self.get_addr().clone();
        self.inner
            .outgoing
            .send(super::TransportEvent::Incoming(msg, transport, source))
            .map_err(|e| e.into())
    }

    pub fn get_addr(&self) -> &SipAddr {
        return &self.inner.addr;
    }

    pub async fn serve_loop(&self, sender: TransportSender) -> Result<()> {
        let incoming = self.inner.clone().incoming.lock().unwrap().take();
        if incoming.is_none() {
            return Err(crate::Error::Error(
                "ChannelTransport::serve_loop called twice".to_string(),
            ));
        }
        let mut incoming = incoming.unwrap();
        while let Some(event) = incoming.recv().await {
            sender.send(event)?;
        }
        Ok(())
    }
}

impl std::fmt::Display for ChannelConnection {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "*:*")
    }
}

impl std::fmt::Debug for ChannelConnection {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "*:*")
    }
}
