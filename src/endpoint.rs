use std::cell::RefCell;
use std::collections::HashMap;
use std::sync::Mutex;
use futures::Stream;
use tracing::info;
use tokio::select;
use crate::Result;

use crate::transaction::{gen_transaction_key, ClientTransaction, ClientTransactionSender, OutgoingReceiver, OutgoingSender, ServerTransaction};
use crate::transport::TransportLayer;

const USER_AGENT: &str = "RustSIP/0.1";

#[derive(Debug)]
pub struct Endpoint {
    user_agent: String,
    transport_layer: TransportLayer,
    client_transactions: Mutex<HashMap<String, ClientTransactionSender>>,
    outgoing_rx: OutgoingReceiver,
    outgoing_tx: OutgoingSender,
}

pub struct EndpointBuilder {
    user_agent: String,
    transport_layer: RefCell<Option<TransportLayer>>,
}

impl EndpointBuilder {
    pub fn new() -> Self {
        EndpointBuilder {
            user_agent: USER_AGENT.to_string(),
            transport_layer: RefCell::new(None),
        }
    }

    pub fn user_agent(&mut self, user_agent: &str) -> &mut Self {
        self.user_agent = user_agent.to_string();
        self
    }

    pub fn transport_layer(&mut self, transport_layer: TransportLayer) -> &mut Self {
        self.transport_layer.replace(Some(transport_layer));
        self
    }

    pub fn build(&self) -> Endpoint {
        let (outgoing_tx, outgoing_rx) = tokio::sync::mpsc::unbounded_channel();
        Endpoint {
            user_agent: self.user_agent.clone(),
            transport_layer: self.transport_layer.borrow_mut().take().unwrap_or(TransportLayer::new()),
            client_transactions: Mutex::new(HashMap::new()),
            outgoing_rx,
            outgoing_tx,
        }
    }
}

impl Endpoint {
    pub async fn serve(&self) {
        info!("Starting UserAgent: {}", self.user_agent);
        select! {
            _ = self.process_timers() => {},
            _ = self.process_outgoing() => {},
        }
    }

    pub fn client_transaction(&self, req: rsip::message::Request) -> Result<ClientTransaction> {       
        let key = gen_transaction_key(&req)?;
        let mut client_transactions = self.client_transactions.lock().unwrap();
        if client_transactions.contains_key(&key) {
            return Err(crate::Error::TransactionError("Transaction already exists.".to_string()));
        }

        let (tx, rx) = tokio::sync::mpsc::unbounded_channel(); 
        let client_tx = ClientTransaction{
            key: key.clone(),
            origin: req,
            receiver: rx,
            outgoing_tx: self.outgoing_tx.clone(),
        };
        client_transactions.insert(key, tx);
        Ok(client_tx)
    }

    pub fn server_transaction(&self) -> impl Stream<Item = Option<ServerTransaction>> {
        futures::stream::empty()
    }
}

impl Endpoint {
    async fn process_timers(&self) {
        todo!()
    }

    async fn process_outgoing(&self) {
        todo!()
    }
}