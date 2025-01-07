use crate::transport::{transport::SipAddr, Transport};
use key::TransactionKey;
use std::time::Duration;
use tokio::sync::mpsc::{UnboundedReceiver, UnboundedSender};

pub mod endpoint;
pub mod key;
pub mod message;
mod timer;
pub mod transaction;
pub use endpoint::EndpointBuilder;
mod tests;
pub struct IncomingRequest {
    pub request: rsip::Request,
    pub transport: Transport,
    pub from: SipAddr,
}

pub type RequestReceiver = UnboundedReceiver<Option<IncomingRequest>>;
pub type RequestSender = UnboundedSender<Option<IncomingRequest>>;

#[derive(Debug, Clone, PartialEq, Copy)]
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

pub enum TransactionTimer {
    TimerA(TransactionKey, Duration),
    TimerB(TransactionKey),
    TimerD(TransactionKey),
    TimerE(TransactionKey),
    TimerF(TransactionKey),
    TimerK(TransactionKey),
    TimerG(TransactionKey, Duration),
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
            TransactionTimer::TimerG(key, _) => key,
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
            TransactionTimer::TimerG(key, duration) => {
                write!(f, "TimerG: {} {}", key, duration.as_millis())
            }
            TransactionTimer::TimerK(key) => write!(f, "TimerK: {}", key),
            TransactionTimer::TimerCleanup(key) => write!(f, "TimerCleanup: {}", key),
        }
    }
}
