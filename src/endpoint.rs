use std::cell::RefCell;
use futures::Stream;
use tracing::{info};
use wasm_bindgen::prelude::wasm_bindgen;

use crate::{transaction::{ClientTransaction, ServerTransaction}, transport::TransportLayer};

const USER_AGENT: &str = "RustSIP/0.1";

#[derive(Debug, PartialEq, Eq)]
pub struct Endpoint {
    user_agent: String,
    transport_layer: TransportLayer,
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
        Endpoint {
            user_agent: self.user_agent.clone(),
            transport_layer: self.transport_layer.borrow_mut().take().unwrap_or(TransportLayer::new()),
        }
    }
}

impl Endpoint {
    pub async fn serve(&self) {
        info!("Starting UserAgent: {}", self.user_agent);        
    }

    pub fn client_transaction(&self, req: rsip::message::Request) -> Box<dyn ClientTransaction> {        
        match req.method {
            rsip::method::Method::Invite => {
                Box::new(crate::transaction::client_invite::ClientInviteTransaction::new(req))
            }
            _ => {
                Box::new(crate::transaction::client_non_invite::ClientNonInviteTransaction::new(req))
            }
        }
    }

    pub fn server_transaction(&self) -> impl Stream<Item = Box<dyn ServerTransaction>> {
        futures::stream::empty()
    }
}

#[wasm_bindgen]
impl Endpoint {

}