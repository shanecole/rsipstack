use super::{
    transaction::{Transaction, TransactionCore, TransactionCoreRef},
    TransactionEvent, TransactionReceiver, TransactionSender, TransportLayer,
};
use crate::Result;
use rsip::SipMessage;
use std::time::Duration;
use tokio::{select, sync::mpsc::unbounded_channel};
use tokio_util::sync::CancellationToken;
use tracing::info;

const USER_AGENT: &str = "rsipstack/0.1";

pub struct EndpointBuilder {
    user_agent: String,
    transport_layer: Option<TransportLayer>,
    cancel_token: Option<CancellationToken>,
    timer_interval: Option<Duration>,
}

pub struct Endpoint {
    core: TransactionCoreRef,
    user_agent: String,
    cancel_token: CancellationToken,
    incoming_receiver: TransactionReceiver,
    incoming_sender: TransactionSender,
}

impl EndpointBuilder {
    pub fn new() -> Self {
        EndpointBuilder {
            user_agent: USER_AGENT.to_string(),
            transport_layer: None,
            cancel_token: None,
            timer_interval: None,
        }
    }

    pub fn user_agent(&mut self, user_agent: &str) -> &mut Self {
        self.user_agent = user_agent.to_string();
        self
    }

    pub fn transport_layer(&mut self, transport_layer: TransportLayer) -> &mut Self {
        self.transport_layer.replace(transport_layer);
        self
    }

    pub fn cancel_token(&mut self, cancel_token: CancellationToken) -> &mut Self {
        self.cancel_token.replace(cancel_token);
        self
    }

    pub fn timer_interval(&mut self, timer_interval: Duration) -> &mut Self {
        self.timer_interval.replace(timer_interval);
        self
    }

    pub fn build(&mut self) -> Endpoint {
        let transport_layer = self
            .transport_layer
            .take()
            .expect("transport_layer is required");

        let cancel_token = self.cancel_token.take().unwrap_or_default();
        let (incoming_sender, incoming_receiver) = unbounded_channel();
        let core = TransactionCore::new(
            transport_layer,
            cancel_token.child_token(),
            self.timer_interval,
            incoming_sender.clone(),
        );

        Endpoint {
            core,
            incoming_receiver,
            incoming_sender,
            user_agent: self.user_agent.clone(),
            cancel_token: self.cancel_token.take().unwrap(),
        }
    }
}

impl Endpoint {
    pub async fn serve(&self) {
        select! {
            _ = self.cancel_token.cancelled() => {
                info!("endpoint cancelled");
            },
            _ = self.core.process_timer() => {
            }
        }
        info!("endpoint shutdown");
    }

    pub fn shutdown(&self) {
        info!("endpoint shutdown requested");
        self.cancel_token.cancel();
        self.incoming_sender.send(TransactionEvent::Terminate).ok(); // ensure the server_transaction loop exits
    }

    pub fn client_transaction(&self, request: rsip::Request) -> Result<Transaction> {
        let key = (&request).try_into()?;
        let tx = Transaction::new_client(key, request, self.core.clone(), None);
        Ok(tx)
    }

    pub async fn server_transaction(&mut self) -> Option<Transaction> {
        match self.incoming_receiver.recv().await? {
            TransactionEvent::Received(SipMessage::Request(req), transport) => {
                let tx = Transaction::new_server(
                    (&req).try_into().ok()?,
                    req,
                    self.core.clone(),
                    transport,
                );
                return Some(tx);
            }
            _ => {}
        }
        None
    }
}
