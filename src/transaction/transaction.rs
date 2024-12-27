use super::timer::Timer;
use super::{key::TransactionKey, message::make_response};
use crate::Result;
use async_trait::async_trait;
use rsip::{Method, Request, Response, SipMessage};
use std::{
    collections::HashMap,
    sync::{Arc, Mutex},
    time::{Duration, Instant},
};
use tokio::sync::mpsc::{unbounded_channel, UnboundedReceiver, UnboundedSender};
use tokio_util::sync::CancellationToken;
use tracing::info;

pub(super) const T1: Duration = Duration::from_millis(500);
pub(super) const T1X64: Duration = Duration::from_secs(32);

pub(crate) struct TransactionCore {
    pub timers: Timer<TransactionTimer>,
    pub transport_layer: TransportLayer,
    pub finished_transactions: Mutex<HashMap<TransactionKey, TransactionHandleRef>>,
    pub transactions: Mutex<HashMap<TransactionKey, TransactionHandleRef>>,
    cancel_token: CancellationToken,
    timer_interval: Duration,
}
pub(super) type TransactionCoreRef = Arc<TransactionCore>;

impl TransactionCore {
    pub fn new(
        transport_layer: TransportLayer,
        cancel_token: CancellationToken,
        timer_interval: Option<Duration>,
    ) -> Arc<Self> {
        Arc::new(TransactionCore {
            timers: Timer::new(),
            transport_layer,
            transactions: Mutex::new(HashMap::new()),
            finished_transactions: Mutex::new(HashMap::new()),
            timer_interval: timer_interval.unwrap_or(Duration::from_millis(20)),
            cancel_token,
        })
    }

    pub(super) async fn process_timer(&self) -> Result<()> {
        while !self.cancel_token.is_cancelled() {
            for t in self.timers.poll(Instant::now()) {
                match t {
                    TransactionTimer::TimerK(key) => {
                        self.transactions.lock().unwrap().remove(&key);
                        self.finished_transactions.lock().unwrap().remove(&key);
                        continue;
                    }
                    _ => {}
                };
                if let Some(handle) = { self.transactions.lock().unwrap().get(t.key()).cloned() } {
                    handle
                        .tu_sender
                        .as_ref()
                        .map(|s| s.send(TransactionEvent::Timer(t)));
                }
            }
            tokio::time::sleep(self.timer_interval).await;
        }
        Ok(())
    }

    fn attach_transaction(&self, key: TransactionKey, tu_sender: TransactionSender) {
        let tx_handle = TransactionHandle {
            tu_sender: Some(tu_sender),
            last_message: None,
        };
        self.transactions
            .lock()
            .unwrap()
            .insert(key.clone(), Arc::new(tx_handle));
    }

    fn detach_transaction(&self, key: TransactionKey, last_message: Option<SipMessage>) {
        self.transactions.lock().unwrap().remove(&key);

        if let Some(msg) = last_message {
            self.timers
                .timeout(T1X64, TransactionTimer::TimerK(key.clone()));
            let handle = TransactionHandle {
                tu_sender: None,
                last_message: Some(msg),
            };
            self.finished_transactions
                .lock()
                .unwrap()
                .insert(key.clone(), Arc::new(handle));
        }
    }
}

#[async_trait]
pub trait TransportHandler {
    fn is_secure(&self) -> bool;
    fn is_reliable(&self) -> bool;
    fn is_stream(&self) -> bool;
    async fn next(&self) -> Result<SipMessage>;
    async fn send(&self, msg: SipMessage) -> Result<()>;
}

pub(super) type Transport = Arc<dyn TransportHandler + Send + Sync>;
#[async_trait]
pub trait TransactionLayerHandler {
    async fn lookup(&self, uri: &rsip::uri::Uri) -> Result<Transport>;
}
pub type TransportLayer = Arc<dyn TransactionLayerHandler + Send + Sync>;

pub(super) type TransactionReceiver = UnboundedReceiver<TransactionEvent>;
pub(super) type TransactionSender = UnboundedSender<TransactionEvent>;

pub enum TransactionState {
    Trying,
    Proceeding,
    Completed,
    Terminated,
}
pub enum TransactionType {
    ClientInvite,
    ClientNonInvite,
    ServerInvite,
    ServerNonInvite,
}

pub(super) enum TransactionEvent {
    Received(SipMessage, Option<Transport>),
    Timer(TransactionTimer),
}

pub enum TransactionTimer {
    TimerA(TransactionKey, Duration),
    TimerB(TransactionKey),
    TimerD(TransactionKey),
    TimerE(TransactionKey),
    TimerF(TransactionKey),
    TimerK(TransactionKey),
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
        }
    }
}

pub(super) struct TransactionHandle {
    tu_sender: Option<TransactionSender>,
    last_message: Option<SipMessage>,
}
type TransactionHandleRef = Arc<TransactionHandle>;

pub struct Transaction {
    pub(super) transaction_type: TransactionType,
    pub(super) core: TransactionCoreRef,
    pub(super) transport: Option<Transport>,
    pub(super) key: TransactionKey,
    pub(super) original: Request,
    pub(super) state: TransactionState,
    pub(super) last_response: Option<Response>,
    pub(super) last_ack: Option<Request>,
    pub(super) tu_receiver: TransactionReceiver,
    pub(super) tu_sender: TransactionSender,
    pub(super) timer_a: Option<u64>,
    pub(super) timer_b: Option<u64>,
    pub(super) timer_d: Option<u64>,
}

impl Transaction {
    fn new(
        transaction_type: TransactionType,
        key: TransactionKey,
        original: Request,
        transport: Option<Transport>,
        core: TransactionCoreRef,
    ) -> Self {
        let (tu_sender, tu_receiver) = unbounded_channel();
        core.attach_transaction(key.clone(), tu_sender.clone());

        Self {
            transaction_type,
            core,
            transport,
            key,
            original,
            state: TransactionState::Trying,
            last_response: None,
            last_ack: None,
            timer_a: None,
            timer_b: None,
            timer_d: None,
            tu_receiver,
            tu_sender,
        }
    }

    pub(super) fn new_client(
        key: TransactionKey,
        original: Request,
        core: TransactionCoreRef,
        transport: Option<Transport>,
    ) -> Self {
        let tx_type = match original.method {
            Method::Invite => TransactionType::ClientInvite,
            _ => TransactionType::ClientNonInvite,
        };
        Transaction::new(tx_type, key, original, transport, core)
    }

    pub async fn send(&mut self) -> Result<()> {
        if let Some(transport) = &self.transport {
            return transport.send(self.original.to_owned().into()).await;
        }

        let transport = self.core.transport_layer.lookup(&self.original.uri).await?;
        self.transport.replace(transport.clone());
        transport.send(self.original.to_owned().into()).await
    }

    pub async fn receive(&mut self) -> Option<SipMessage> {
        while let Some(event) = self.tu_receiver.recv().await {
            match event {
                TransactionEvent::Received(msg, transport) => {
                    if let Some(msg) = match msg {
                        SipMessage::Request(req) => self.on_received_request(req, transport).await,
                        SipMessage::Response(resp) => {
                            self.on_received_response(resp, transport).await
                        }
                    } {
                        return Some(msg);
                    }
                }
                TransactionEvent::Timer(t) => {
                    self.on_timer(t).await.ok();
                }
            }
        }
        None
    }
}

impl Transaction {
    fn inform_tu_response(&mut self, response: Response) -> Result<()> {
        self.tu_sender
            .send(TransactionEvent::Received(
                SipMessage::Response(response),
                None,
            ))
            .map_err(|e| crate::Error::TransactionError(e.to_string()))
    }

    async fn on_received_request(
        &mut self,
        req: Request,
        transport: Option<Transport>,
    ) -> Option<SipMessage> {
        None
    }

    async fn on_received_response(
        &mut self,
        resp: Response,
        transport: Option<Transport>,
    ) -> Option<SipMessage> {
        None
    }

    async fn on_timer(&mut self, timer: TransactionTimer) -> Result<()> {
        match self.state {
            TransactionState::Trying => {
                if let TransactionTimer::TimerA(key, duration) = timer {
                    // Resend the INVITE request
                    if let Some(transport) = &self.transport {
                        transport.send(self.original.to_owned().into()).await?;
                    }
                    // Restart Timer A with an upper limit
                    let duration = (duration * 2).min(T1X64);
                    let timer_a = self
                        .core
                        .timers
                        .timeout(duration, TransactionTimer::TimerA(key, duration));
                    self.timer_a.replace(timer_a);
                } else if let TransactionTimer::TimerB(_) = timer {
                    // Inform TU about timeout
                    let timeout_response =
                        make_response(&self.original, rsip::StatusCode::RequestTimeout, None);
                    self.inform_tu_response(timeout_response)?;
                    self.transition(TransactionState::Terminated);
                }
            }
            TransactionState::Proceeding => {
                if let TransactionTimer::TimerB(_) = timer {
                    // Inform TU about timeout
                    let timeout_response =
                        make_response(&self.original, rsip::StatusCode::RequestTimeout, None);
                    self.inform_tu_response(timeout_response)?;
                    self.transition(TransactionState::Terminated);
                }
            }
            TransactionState::Completed => {
                if let TransactionTimer::TimerD(_) = timer {
                    self.transition(TransactionState::Terminated);
                }
            }
            _ => {}
        }
        Ok(())
    }

    fn transition(&mut self, state: TransactionState) {
        match state {
            TransactionState::Proceeding => {
                self.timer_a.take().map(|id| self.core.timers.cancel(id));
                // start Timer B
                let timer_b = self
                    .core
                    .timers
                    .timeout(T1X64, TransactionTimer::TimerB(self.key.clone()));
                self.timer_b.replace(timer_b);
            }
            TransactionState::Completed => {
                self.timer_a.take().map(|id| self.core.timers.cancel(id));
                self.timer_b.take().map(|id| self.core.timers.cancel(id));
                // start Timer D
                let timer_d = self
                    .core
                    .timers
                    .timeout(T1X64, TransactionTimer::TimerD(self.key.clone()));
                self.timer_d.replace(timer_d);
            }
            TransactionState::Terminated => {
                self.cleanup_timer();
                // Inform TU about termination
                let last_message = match self.transaction_type {
                    TransactionType::ClientInvite => {
                        self.last_ack.take().map(|r| SipMessage::Request(r))
                    }
                    TransactionType::ServerNonInvite => {
                        self.last_response.take().map(|r| SipMessage::Response(r))
                    }
                    _ => None,
                };
                self.core.detach_transaction(self.key.clone(), last_message);
            }
            _ => {}
        }
        self.state = state;
    }

    fn cleanup_timer(&mut self) {
        self.timer_a.take().map(|id| self.core.timers.cancel(id));
        self.timer_b.take().map(|id| self.core.timers.cancel(id));
        self.timer_d.take().map(|id| self.core.timers.cancel(id));
    }
}

impl Drop for Transaction {
    fn drop(&mut self) {
        // cancel All timers
        self.cleanup_timer();
    }
}
