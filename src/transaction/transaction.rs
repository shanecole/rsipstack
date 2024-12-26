use crate::Result;
use async_trait::async_trait;
use rsip::{Method, Request, Response, SipMessage};
use std::sync::Arc;

use super::client_invite::ClientInviteHandler;
use super::client_non_invite::ClientNonInviteHandler;

use super::key::TransactionKey;
use super::server_invite::ServerInviteHandler;
use super::server_non_invite::ServerNonInviteHandler;
use super::timer::Timer;

pub(super) struct TransactionCore {
    pub timers: Timer<TransactionTimer>,
    pub transport_layer: TransportLayer,
}
pub(super) type TransactionCoreRef = Arc<TransactionCore>;

impl TransactionCore {
    pub fn new(transport_layer: TransportLayer) -> Arc<Self> {
        Arc::new(TransactionCore {
            timers: Timer::new(),
            transport_layer,
        })
    }
}

#[async_trait]
pub trait TransportHandler {
    fn is_secure(&self) -> bool;
    fn is_reliable(&self) -> bool;
    fn is_stream(&self) -> bool;
    async fn next(&self) -> Result<SipMessage>;
    async fn send(&self, msg: &SipMessage) -> Result<()>;
}

type Transport = Arc<dyn TransportHandler + Send + Sync>;
#[async_trait]
pub trait TransactionLayerHandler {
    async fn lookup(&self, msg: &SipMessage) -> Result<Transport>;
}
pub type TransportLayer = Arc<dyn TransactionLayerHandler + Send + Sync>;

#[derive(Debug)]
pub enum TransactionState {
    Calling,
    Proceeding,
    Completed,
    Terminated,
}

pub const T1: u64 = 500;
pub const T2: u64 = 4000;
pub const T4: u64 = 5000;

pub enum TransactionTimer {
    TimerA(TransactionKey), // 500ms
    TimerB(TransactionKey), // 64 * T1
    TimerD(TransactionKey), // 0 * T1
    TimerE(TransactionKey), // 0 * T1
    TimerF(TransactionKey), // 64 * T1
    TimerK(TransactionKey), // 0 * T4
}

impl TransactionTimer {
    pub fn key(&self) -> &TransactionKey {
        match self {
            TransactionTimer::TimerA(key) => key,
            TransactionTimer::TimerB(key) => key,
            TransactionTimer::TimerD(key) => key,
            TransactionTimer::TimerE(key) => key,
            TransactionTimer::TimerF(key) => key,
            TransactionTimer::TimerK(key) => key,
        }
    }
}

impl std::fmt::Display for TransactionTimer {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            TransactionTimer::TimerA(key) => write!(f, "TimerA: {}", key),
            TransactionTimer::TimerB(key) => write!(f, "TimerB: {}", key),
            TransactionTimer::TimerD(key) => write!(f, "TimerD: {}", key),
            TransactionTimer::TimerE(key) => write!(f, "TimerE: {}", key),
            TransactionTimer::TimerF(key) => write!(f, "TimerF: {}", key),
            TransactionTimer::TimerK(key) => write!(f, "TimerK: {}", key),
        }
    }
}

pub(crate) struct TransactionInner {
    pub core: TransactionCoreRef,
    pub transport: Option<Transport>,
    pub transaction_state: TransactionState,
    pub original_request: Option<Request>,
    pub last_response: Option<Response>,
}

pub(crate) type TransactionInnerRef = Arc<TransactionInner>;

#[derive(Clone)]
pub enum Transaction {
    ClientInvite(ClientInviteHandler),
    ClientNonInvite(ClientNonInviteHandler),
    ServerInvite(ServerInviteHandler),
    ServerNonInvite(ServerNonInviteHandler),
}

impl Transaction {
    pub(super) fn new_client(
        method: Method,
        transport: Option<Transport>,
        core: TransactionCoreRef,
    ) -> Self {
        let inner = TransactionInner {
            core,
            transport,
            transaction_state: TransactionState::Calling,
            original_request: None,
            last_response: None,
        };
        let inner_ref = Arc::new(inner);
        match method {
            Method::Invite => Transaction::ClientInvite(ClientInviteHandler {
                inner: inner_ref,
                last_ack: None,
            }),
            _ => Transaction::ClientNonInvite(ClientNonInviteHandler { inner: inner_ref }),
        }
    }

    pub(super) fn new_server(
        method: Method,
        transport: Option<Transport>,
        core: TransactionCoreRef,
    ) -> Self {
        let inner = TransactionInner {
            core,
            transport,
            transaction_state: TransactionState::Calling,
            original_request: None,
            last_response: None,
        };
        let inner_ref = Arc::new(inner);
        match method {
            Method::Invite => Transaction::ServerInvite(ServerInviteHandler {
                inner: inner_ref,
                last_ack: None,
            }),
            _ => Transaction::ServerNonInvite(ServerNonInviteHandler { inner: inner_ref }),
        }
    }
}

impl Transaction {
    pub(crate) async fn on_timer(&self, timer: &TransactionTimer) -> Result<()> {
        match self {
            Transaction::ClientInvite(handler) => handler.on_timer(timer).await,
            Transaction::ClientNonInvite(handler) => handler.on_timer(timer).await,
            Transaction::ServerInvite(handler) => handler.on_timer(timer).await,
            Transaction::ServerNonInvite(handler) => handler.on_timer(timer).await,
        }
    }

    pub(crate) async fn on_received(
        &self,
        msg: &SipMessage,
        transport: Option<Transport>,
    ) -> Result<()> {
        Ok(())
    }
}
