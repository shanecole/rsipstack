use crate::transport::{connection::SipAddr, SipConnection};
use key::TransactionKey;
use std::time::Duration;
use tokio::sync::mpsc::{UnboundedReceiver, UnboundedSender};
use transaction::Transaction;
use uuid::Uuid;

pub mod endpoint;
pub mod key;
pub mod message;
mod timer;
pub mod transaction;
pub use endpoint::Endpoint;
pub use endpoint::EndpointBuilder;
#[cfg(test)]
mod tests;

pub const TO_TAG_LEN: usize = 8;
pub const BRANCH_LEN: usize = 12;
pub const CNONCE_LEN: usize = 8;

pub struct IncomingRequest {
    pub request: rsip::Request,
    pub connection: SipConnection,
    pub from: SipAddr,
}

pub type TransactionReceiver = UnboundedReceiver<Transaction>;
pub type TransactionSender = UnboundedSender<Transaction>;

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

pub fn make_via_branch() -> rsip::Param {
    rsip::Param::Branch(format!("z9hG4bK{}", random_text(BRANCH_LEN)).into())
}

pub fn make_call_id(domain: Option<&str>) -> rsip::headers::CallId {
    format!("{}@{}", Uuid::new_v4(), domain.unwrap_or("restsend.com")).into()
}

pub fn make_tag() -> rsip::param::Tag {
    random_text(TO_TAG_LEN).into()
}

#[cfg(not(target_family = "wasm"))]
pub fn random_text(count: usize) -> String {
    use rand::Rng;
    rand::thread_rng()
        .sample_iter(rand::distributions::Alphanumeric)
        .take(count)
        .map(char::from)
        .collect::<String>()
}

#[cfg(target_family = "wasm")]
pub fn random_text(count: usize) -> String {
    (0..count)
        .map(|_| {
            let r = js_sys::Math::random();
            let c = (r * 16.0) as u8;
            if c < 10 {
                (c + 48) as char
            } else {
                (c + 87) as char
            }
        })
        .collect()
}
