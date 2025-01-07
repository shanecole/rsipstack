use super::{
    transport::{SipAddr, TransportReceiver, TransportSender},
    Transport,
};
use crate::Result;
use std::sync::{Arc, Mutex};

struct ChannelTransportInner {
    incoming: Mutex<Option<TransportReceiver>>,
    outgoing: TransportSender,
    addr: SipAddr,
}

#[derive(Clone)]
pub struct ChannelTransport {
    inner: Arc<ChannelTransportInner>,
}

impl ChannelTransport {
    pub async fn create_connection(
        incoming: TransportReceiver,
        outgoing: TransportSender,
        addr: SipAddr,
    ) -> Result<Self> {
        let t = ChannelTransport {
            inner: Arc::new(ChannelTransportInner {
                incoming: Mutex::new(Some(incoming)),
                outgoing,
                addr,
            }),
        };
        Ok(t)
    }

    pub async fn send(&self, msg: rsip::SipMessage) -> crate::Result<()> {
        let transport = Transport::Channel(self.clone());
        let source = self.get_addr().clone();
        self.inner
            .outgoing
            .send(super::TransportEvent::IncomingMessage(
                msg, transport, source,
            ))
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

impl std::fmt::Display for ChannelTransport {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "*:*")
    }
}

impl std::fmt::Debug for ChannelTransport {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "*:*")
    }
}
