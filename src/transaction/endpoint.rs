use super::{
    key::TransactionKey,
    transaction::{Transaction, TransactionCore, TransactionCoreRef, TransportLayer},
};
use crate::Result;
use std::{
    collections::HashMap,
    time::{Duration, Instant},
};
use tokio::select;
use tokio_util::sync::CancellationToken;
use tracing::info;

const USER_AGENT: &str = "rustsip/0.1";

pub struct EndpointBuilder {
    user_agent: String,
    transport_layer: Option<TransportLayer>,
    cancel_token: Option<CancellationToken>,
    timer_interval: Option<Duration>,
}

pub struct Endpoint {
    core: TransactionCoreRef,
    user_agent: String,
    transactions: HashMap<TransactionKey, Transaction>,
    cancel_token: CancellationToken,
    timer_interval: Duration,
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

        let core = TransactionCore::new(transport_layer);

        Endpoint {
            core,
            user_agent: self.user_agent.clone(),
            cancel_token: self.cancel_token.take().unwrap_or_default(),
            transactions: HashMap::new(),
            timer_interval: self
                .timer_interval
                .take()
                .unwrap_or(Duration::from_millis(20)),
        }
    }
}

impl Endpoint {
    pub async fn serve(&self) {
        select! {
            _ = self.cancel_token.cancelled() => {
                info!("endpoint cancelled");
            },
            _ = self.process_timer() => {
            }
        }
        info!("endpoint shutdown");
    }

    pub fn shutdown(&self) {
        self.cancel_token.cancel();
    }
}

impl Endpoint {
    async fn process_timer(&self) -> Result<()> {
        loop {
            for t in self.core.timers.poll(Instant::now()) {
                if let Some(transaction) = self.transactions.get(t.key()) {
                    let r = transaction.on_timer(&t).await;
                    if let Err(e) = r {
                        info!("process_timer {} error: {:?}", t, e);
                    }
                }
            }
            tokio::time::sleep(self.timer_interval).await;
        }
    }
}
