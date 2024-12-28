use super::{
    transaction::{Transaction, TransactionCore, TransactionCoreRef},
    RequestReceiver, RequestSender, Transport,
};
use crate::{transport::TransportLayer, Result, USER_AGENT};
use std::{sync::Mutex, time::Duration};
use tokio::{select, sync::mpsc::unbounded_channel};
use tokio_util::sync::CancellationToken;
use tracing::info;

pub struct EndpointBuilder {
    user_agent: String,
    transport_layer: Option<TransportLayer>,
    cancel_token: Option<CancellationToken>,
    timer_interval: Option<Duration>,
}

pub struct Endpoint {
    core: TransactionCoreRef,
    cancel_token: CancellationToken,
    incoming_sender: Mutex<Option<RequestSender>>,
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
        let transport_layer = self.transport_layer.take().unwrap_or_default();

        let cancel_token = self.cancel_token.take().unwrap_or_default();
        let core = TransactionCore::new(
            self.user_agent.clone(),
            transport_layer,
            cancel_token.child_token(),
            self.timer_interval,
        );

        Endpoint {
            core,
            incoming_sender: Mutex::new(None),
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
            _ = self.core.serve() => {},

        }
        info!("endpoint shutdown");
    }

    pub fn shutdown(&self) {
        info!("endpoint shutdown requested");
        self.cancel_token.cancel();
        self.incoming_sender
            .lock()
            .unwrap()
            .as_ref()
            .map(|incoming| incoming.send(None).ok()); // send None to incoming_sender to shutdown
    }

    pub fn client_transaction(&self, request: rsip::Request) -> Result<Transaction> {
        let key = (&request).try_into()?;
        let tx = Transaction::new_client(key, request, self.core.clone(), None);
        Ok(tx)
    }

    pub fn server_transaction(
        &self,
        request: rsip::Request,
        transport: Transport,
    ) -> Result<Transaction> {
        let key = (&request).try_into()?;
        let tx = Transaction::new_server(key, request, self.core.clone(), Some(transport));
        Ok(tx)
    }

    //
    // get incoming requests from the endpoint
    //
    pub fn incoming_requests(&self) -> RequestReceiver {
        let (tx, rx) = unbounded_channel();
        self.core.attach_incoming_sender(Some(tx.clone()));
        self.incoming_sender.lock().unwrap().replace(tx);
        rx
    }
}
