use super::{
    transport::{TransportReceiver, TransportSender},
    Transport,
};
use crate::{transport::TransportEvent, Result};
use std::{
    collections::HashMap,
    net::SocketAddr,
    sync::{Arc, Mutex},
};
use tokio::{select, sync::mpsc::unbounded_channel};
use tokio_util::sync::CancellationToken;
use tracing::info;

#[derive(Default)]
pub struct TransportLayerInner {
    cancel_token: CancellationToken,
    transports: Mutex<HashMap<SocketAddr, Transport>>,
    transport_sender: Mutex<Option<TransportSender>>,
}
#[derive(Default)]

pub struct TransportLayer {
    inner: Arc<TransportLayerInner>,
}

impl TransportLayer {
    pub fn new(cancel_token: CancellationToken) -> Self {
        let inner = TransportLayerInner {
            cancel_token,
            transports: Mutex::new(HashMap::new()),
            transport_sender: Mutex::new(None),
        };
        Self {
            inner: Arc::new(inner),
        }
    }

    pub fn add_transport(&self, transport: Transport) {
        self.inner.add_transport(transport)
    }

    pub fn del_transport(&self, addr: &SocketAddr) {
        self.inner.del_transport(addr)
    }

    pub async fn lookup(&self, uri: &rsip::uri::Uri) -> Result<Transport> {
        self.inner.lookup(uri).await
    }

    pub async fn serve(&self, sender: TransportSender) -> Result<()> {
        self.inner.serve(sender).await
    }
}

impl TransportLayerInner {
    pub fn add_transport(&self, transport: Transport) {
        self.transports
            .lock()
            .unwrap()
            .insert(transport.get_addr().to_owned(), transport);
    }

    pub fn del_transport(&self, addr: &SocketAddr) {
        self.transports.lock().unwrap().remove(addr);
    }

    async fn lookup(&self, uri: &rsip::uri::Uri) -> Result<Transport> {
        let target = uri.host_with_port.clone().try_into()?;
        let transports = self.transports.lock().unwrap();
        if let Some(transport) = transports.get(&target) {
            return Ok(transport.clone());
        }
        Err(crate::Error::TransportLayerError(
            format!("transport not found: {}", target),
            target,
        ))
    }

    async fn serve(&self, sender: TransportSender) -> Result<()> {
        let transports = self.transports.lock().unwrap().clone();

        let (inner_tx, mut inner_rx) = unbounded_channel();
        self.transport_sender
            .lock()
            .unwrap()
            .replace(inner_tx.clone());

        for (_, transport) in transports {
            let sender = sender.clone();
            let sub_token = self.cancel_token.child_token();
            let sender_clone = sender.clone();
            let inner_tx_clone = inner_tx.clone();

            tokio::spawn(async move {
                select! {
                    _ = sub_token.cancelled() => { }
                    _ = transport.serve_loop(sender_clone.clone()) => {
                    }
                }
                inner_tx_clone
                    .send(TransportEvent::TransportClosed(transport.clone()))
                    .ok(); // for transport_inner
                sender_clone
                    .send(TransportEvent::TransportClosed(transport))
                    .ok();
            });
        }

        let inner_transport_loop = async move {
            while let Some(event) = inner_rx.recv().await {
                match event {
                    TransportEvent::TransportClosed(transport) => {
                        info!("transport closed: {}", transport.get_addr());
                        self.transports.lock().unwrap().remove(transport.get_addr());
                    }
                    _ => {}
                }
            }
        };

        select! {
             _ = inner_transport_loop => { }
             _ = self.cancel_token.cancelled() => { }
        }
        Ok(())
    }
}
