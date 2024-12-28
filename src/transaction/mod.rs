use crate::Result;
use async_trait::async_trait;
use key::TransactionKey;
use rsip::SipMessage;
use std::{sync::Arc, time::Duration};
use tokio::sync::mpsc::{UnboundedReceiver, UnboundedSender};

pub mod endpoint;
pub mod key;
pub mod message;
mod timer;
pub mod transaction;
pub use endpoint::EndpointBuilder;

const USER_AGENT: &str = "rsipstack/0.1";

#[async_trait]
pub trait TransportHandler {
    fn is_secure(&self) -> bool;
    fn is_reliable(&self) -> bool;
    fn is_stream(&self) -> bool;
    async fn next(&self) -> Result<SipMessage>;
    async fn send(&self, msg: SipMessage) -> Result<()>;
}

pub type Transport = Arc<dyn TransportHandler + Send + Sync>;
#[async_trait]
pub trait TransactionLayerHandler {
    async fn lookup(&self, uri: &rsip::uri::Uri) -> Result<Transport>;
}
pub type TransportLayer = Arc<dyn TransactionLayerHandler + Send + Sync>;

pub struct IncomingRequest {
    pub request: rsip::Request,
    pub transport: Transport,
}

pub type RequestReceiver = UnboundedReceiver<Option<IncomingRequest>>;
pub type RequestSender = UnboundedSender<Option<IncomingRequest>>;

#[derive(Debug, Clone, PartialEq)]
pub enum TransactionState {
    Calling,
    Trying,
    Proceeding,
    Completed,
    Confirmed,
    Terminated,
}
#[derive(Debug, PartialEq)]
pub enum TransactionType {
    ClientInvite,
    ClientNonInvite,
    ServerInvite,
    ServerNonInvite,
}

pub(super) enum TransactionTimer {
    TimerA(TransactionKey, Duration),
    TimerB(TransactionKey),
    TimerD(TransactionKey),
    TimerE(TransactionKey),
    TimerF(TransactionKey),
    TimerK(TransactionKey),
    TimerCleanup(TransactionKey),
}

impl TransactionTimer {
    pub fn key(&self) -> &TransactionKey {
        match self {
            TransactionTimer::TimerA(key, _) => key,
            TransactionTimer::TimerB(key) => key,
            TransactionTimer::TimerD(key) => key,
            TransactionTimer::TimerE(key) => key,
            TransactionTimer::TimerF(key) => key,
            TransactionTimer::TimerK(key) => key,
            TransactionTimer::TimerCleanup(key) => key,
        }
    }
}

impl std::fmt::Display for TransactionTimer {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            TransactionTimer::TimerA(key, duration) => {
                write!(f, "TimerA: {} {}", key, duration.as_millis())
            }
            TransactionTimer::TimerB(key) => write!(f, "TimerB: {}", key),
            TransactionTimer::TimerD(key) => write!(f, "TimerD: {}", key),
            TransactionTimer::TimerE(key) => write!(f, "TimerE: {}", key),
            TransactionTimer::TimerF(key) => write!(f, "TimerF: {}", key),
            TransactionTimer::TimerK(key) => write!(f, "TimerK: {}", key),
            TransactionTimer::TimerCleanup(key) => write!(f, "TimerCleanup: {}", key),
        }
    }
}
