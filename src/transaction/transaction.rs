use super::endpoint::EndpointInnerRef;
use super::key::TransactionKey;
use super::{TransactionState, TransactionTimer, TransactionType, Transport};
use crate::{Error, Result};
use rsip::{Method, Request, Response, SipMessage};
use std::time::Duration;
use tokio::sync::mpsc::{unbounded_channel, UnboundedReceiver, UnboundedSender};
use tracing::{trace, warn};

pub const T1: Duration = Duration::from_millis(500);
pub const T1X64: Duration = Duration::from_secs(64 * 500);
pub const T4: Duration = Duration::from_secs(4); // server invite only
pub const TIMER_INTERVAL: Duration = Duration::from_millis(20);

pub type TransactionEventReceiver = UnboundedReceiver<TransactionEvent>;
pub type TransactionEventSender = UnboundedSender<TransactionEvent>;
pub enum TransactionEvent {
    Received(SipMessage, Option<Transport>),
    Timer(TransactionTimer),
    Terminate,
}

pub struct Transaction {
    pub transaction_type: TransactionType,
    pub key: TransactionKey,
    pub original: Request,
    pub state: TransactionState,
    pub endpoint_inner: EndpointInnerRef,
    pub transport: Option<Transport>,
    pub last_response: Option<Response>,
    pub last_ack: Option<Request>,
    pub tu_receiver: TransactionEventReceiver,
    pub tu_sender: TransactionEventSender,
    pub timer_a: Option<u64>,
    pub timer_b: Option<u64>,
    pub timer_d: Option<u64>,
    pub timer_k: Option<u64>, // server invite only
    pub timer_g: Option<u64>, // server invite only
}

impl Transaction {
    fn new(
        transaction_type: TransactionType,
        key: TransactionKey,
        original: Request,
        transport: Option<Transport>,
        endpoint_inner: EndpointInnerRef,
    ) -> Self {
        let (tu_sender, tu_receiver) = unbounded_channel();
        Self {
            transaction_type,
            endpoint_inner,
            transport,
            key,
            original,
            state: TransactionState::Calling,
            last_response: None,
            last_ack: None,
            timer_a: None,
            timer_b: None,
            timer_d: None,
            timer_k: None,
            timer_g: None,
            tu_receiver,
            tu_sender,
        }
    }

    pub fn new_client(
        key: TransactionKey,
        original: Request,
        endpoint_inner: EndpointInnerRef,
        transport: Option<Transport>,
    ) -> Self {
        let tx_type = match original.method {
            Method::Invite => TransactionType::ClientInvite,
            _ => TransactionType::ClientNonInvite,
        };
        Transaction::new(tx_type, key, original, transport, endpoint_inner)
    }

    pub fn new_server(
        key: TransactionKey,
        original: Request,
        endpoint_inner: EndpointInnerRef,
        transport: Option<Transport>,
    ) -> Self {
        let tx_type = match original.method {
            Method::Invite => TransactionType::ServerInvite,
            _ => TransactionType::ServerNonInvite,
        };
        Transaction::new(tx_type, key, original, transport, endpoint_inner)
    }
    // send client request
    pub async fn send(&mut self) -> Result<()> {
        match self.transaction_type {
            TransactionType::ClientInvite | TransactionType::ClientNonInvite => {}
            _ => {
                return Err(Error::TransactionError(
                    "send is only valid for client transactions".to_string(),
                    self.key.clone(),
                ));
            }
        }

        if let None = self.transport {
            let transport = self
                .endpoint_inner
                .transport_layer
                .lookup(&self.original.uri)
                .await?;
            self.transport.replace(transport.clone());
        }

        let transport = self.transport.as_ref().ok_or(Error::TransactionError(
            "no transport found".to_string(),
            self.key.clone(),
        ))?;

        transport.send(self.original.to_owned().into()).await?;
        self.endpoint_inner
            .attach_transaction(&self.key, self.tu_sender.clone());
        self.transition(TransactionState::Trying).map(|_| ())
    }

    // send server response
    pub async fn respond(&mut self, response: Response) -> Result<()> {
        match self.transaction_type {
            TransactionType::ServerInvite | TransactionType::ServerNonInvite => {}
            _ => {
                return Err(Error::TransactionError(
                    "respond is only valid for server transactions".to_string(),
                    self.key.clone(),
                ));
            }
        }

        let new_state = match response.status_code.kind() {
            rsip::StatusCodeKind::Provisional => match response.status_code {
                rsip::StatusCode::Trying => TransactionState::Trying,
                _ => TransactionState::Proceeding,
            },
            _ => match self.transaction_type {
                TransactionType::ServerInvite => TransactionState::Completed,
                _ => TransactionState::Terminated,
            },
        };
        // check an transition to new state
        self.can_transition(&new_state)?;

        let transport = self.transport.as_ref().ok_or(Error::TransactionError(
            "no transport found".to_string(),
            self.key.clone(),
        ))?;
        transport.send(response.to_owned().into()).await?;
        self.last_response.replace(response);
        self.transition(new_state).map(|_| ())
    }

    fn can_transition(&self, target: &TransactionState) -> Result<()> {
        match (self.state, target) {
            (TransactionState::Calling, TransactionState::Trying)
            | (TransactionState::Trying, TransactionState::Trying) // retransmission
            | (TransactionState::Trying, TransactionState::Proceeding)
            | (TransactionState::Trying, TransactionState::Completed)
            | (TransactionState::Trying, TransactionState::Confirmed)
            | (TransactionState::Trying, TransactionState::Terminated)
            | (TransactionState::Proceeding, TransactionState::Completed)
            | (TransactionState::Proceeding, TransactionState::Confirmed)
            | (TransactionState::Proceeding, TransactionState::Terminated)
            | (TransactionState::Completed, TransactionState::Confirmed)
            | (TransactionState::Completed, TransactionState::Terminated)
            | (TransactionState::Confirmed, TransactionState::Terminated) => Ok(()),
            _ => {
                return Err(Error::TransactionError(
                    format!(
                        "invalid state transition from {:?} to {:?}",
                        self.state, target
                    ),
                    self.key.clone(),
                ));
            }
        }
    }

    pub async fn send_ack(&mut self, ack: Request) -> Result<()> {
        if self.transaction_type != TransactionType::ClientInvite {
            return Err(Error::TransactionError(
                "send_ack is only valid for client invite transactions".to_string(),
                self.key.clone(),
            ));
        }

        let transport = self.transport.as_ref().ok_or(Error::TransactionError(
            "no transport found".to_string(),
            self.key.clone(),
        ))?;

        match self.state {
            TransactionState::Completed => {} // must be in completed state, to send ACK
            _ => {
                return Err(Error::TransactionError(
                    "invalid state for sending ACK".to_string(),
                    self.key.clone(),
                ));
            }
        }

        transport.send(ack.to_owned().into()).await?;
        self.last_ack.replace(ack);
        // client send ack and transition to Terminated
        self.transition(TransactionState::Terminated).map(|_| ())
    }

    pub async fn receive(&mut self) -> Option<SipMessage> {
        while let Some(event) = self.tu_receiver.recv().await {
            match event {
                TransactionEvent::Received(msg, transport) => {
                    if let Some(msg) = match msg {
                        SipMessage::Request(req) => self.on_received_request(req, transport).await,
                        SipMessage::Response(resp) => self.on_received_response(resp).await,
                    } {
                        return Some(msg);
                    }
                }
                TransactionEvent::Timer(t) => {
                    self.on_timer(t).await.ok();
                }
                TransactionEvent::Terminate => {
                    return None;
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
            .map_err(|e| Error::TransactionError(e.to_string(), self.key.clone()))
    }

    async fn on_received_request(
        &mut self,
        req: Request,
        transport: Option<Transport>,
    ) -> Option<SipMessage> {
        match self.transaction_type {
            TransactionType::ClientInvite | TransactionType::ClientNonInvite => return None,
            _ => {}
        }
        if self.transport.is_none() {
            self.transport = transport;
        }
        match self.state {
            TransactionState::Calling => {
                // auto respond 100 Trying
                let response =
                    self.endpoint_inner
                        .make_response(&req, rsip::StatusCode::Trying, None);
                self.respond(response).await.ok();
                return Some(SipMessage::Request(req));
            }
            TransactionState::Trying | TransactionState::Proceeding => {
                // retransmission of last response
                if let Some(last_response) = &self.last_response {
                    self.respond(last_response.to_owned()).await.ok();
                } else {
                    warn!("received request before sending response");
                    return Some(SipMessage::Request(req));
                }
            }
            TransactionState::Completed => {
                if req.method == Method::Ack {
                    self.transition(TransactionState::Confirmed).ok();
                    return Some(SipMessage::Request(req));
                }
            }
            _ => {}
        }
        None
    }

    async fn on_received_response(&mut self, resp: Response) -> Option<SipMessage> {
        match self.transaction_type {
            TransactionType::ServerInvite | TransactionType::ServerNonInvite => return None,
            _ => {}
        }

        let new_state = match resp.status_code.kind() {
            rsip::StatusCodeKind::Provisional => {
                if resp.status_code == rsip::StatusCode::Trying {
                    TransactionState::Trying
                } else {
                    TransactionState::Proceeding
                }
            }
            rsip::StatusCodeKind::Successful => {
                if self.transaction_type == TransactionType::ClientInvite {
                    TransactionState::Confirmed
                } else {
                    TransactionState::Terminated
                }
            }
            _ => TransactionState::Terminated,
        };

        self.can_transition(&new_state).ok()?;
        if self.state == new_state {
            // ignore duplicate response
            return None;
        }

        self.transition(new_state).ok();
        self.last_response.replace(resp.clone());
        return Some(SipMessage::Response(resp));
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
                        .endpoint_inner
                        .timers
                        .timeout(duration, TransactionTimer::TimerA(key, duration));
                    self.timer_a.replace(timer_a);
                } else if let TransactionTimer::TimerB(_) = timer {
                    self.transition(TransactionState::Terminated)?;
                    // Inform TU about timeout
                    let timeout_response = self.endpoint_inner.make_response(
                        &self.original,
                        rsip::StatusCode::RequestTimeout,
                        None,
                    );
                    self.inform_tu_response(timeout_response)?;
                }
            }
            TransactionState::Proceeding => {
                if let TransactionTimer::TimerB(_) = timer {
                    self.transition(TransactionState::Terminated)?;
                    // Inform TU about timeout
                    let timeout_response = self.endpoint_inner.make_response(
                        &self.original,
                        rsip::StatusCode::RequestTimeout,
                        None,
                    );
                    self.inform_tu_response(timeout_response)?;
                }
            }
            TransactionState::Completed => {
                if let TransactionTimer::TimerG(key, duration) = timer {
                    // resend the response
                    if let Some(last_response) = &self.last_response {
                        if let Some(transport) = &self.transport {
                            transport.send(last_response.to_owned().into()).await?;
                        }
                    }
                    // restart Timer G with an upper limit
                    let duration = (duration * 2).min(T1X64);
                    let timer_g = self
                        .endpoint_inner
                        .timers
                        .timeout(duration, TransactionTimer::TimerG(key, duration));
                    self.timer_g.replace(timer_g);
                } else if let TransactionTimer::TimerD(_) = timer {
                    self.transition(TransactionState::Terminated)?;
                }
            }
            TransactionState::Confirmed => {
                if let TransactionTimer::TimerK(_) = timer {
                    self.transition(TransactionState::Terminated)?;
                }
            }
            _ => {}
        }
        Ok(())
    }

    fn transition(&mut self, state: TransactionState) -> Result<TransactionState> {
        if self.state == state {
            return Ok(self.state.clone());
        }
        match state {
            TransactionState::Calling => {
                // not state can transition to Calling
            }
            TransactionState::Trying => {
                let transport = self.transport.as_ref().ok_or(Error::TransactionError(
                    "no transport found".to_string(),
                    self.key.clone(),
                ))?;

                if !transport.is_reliable() {
                    self.timer_a
                        .take()
                        .map(|id| self.endpoint_inner.timers.cancel(id));
                    self.timer_a.replace(
                        self.endpoint_inner
                            .timers
                            .timeout(T1, TransactionTimer::TimerA(self.key.clone(), T1)),
                    );
                }
                self.timer_b
                    .take()
                    .map(|id| self.endpoint_inner.timers.cancel(id));
                self.timer_b.replace(
                    self.endpoint_inner
                        .timers
                        .timeout(T1X64, TransactionTimer::TimerB(self.key.clone())),
                );
            }
            TransactionState::Proceeding => {
                self.timer_a
                    .take()
                    .map(|id| self.endpoint_inner.timers.cancel(id));
                // start Timer B
                let timer_b = self
                    .endpoint_inner
                    .timers
                    .timeout(T1X64, TransactionTimer::TimerB(self.key.clone()));
                self.timer_b.replace(timer_b);
            }
            TransactionState::Completed => {
                self.timer_a
                    .take()
                    .map(|id| self.endpoint_inner.timers.cancel(id));
                self.timer_b
                    .take()
                    .map(|id| self.endpoint_inner.timers.cancel(id));

                if self.transaction_type == TransactionType::ServerInvite {
                    // start Timer G for server invite only
                    let transport = self.transport.as_ref().ok_or(Error::TransactionError(
                        "no transport found".to_string(),
                        self.key.clone(),
                    ))?;
                    if !transport.is_reliable() {
                        let timer_g = self
                            .endpoint_inner
                            .timers
                            .timeout(T1, TransactionTimer::TimerG(self.key.clone(), T1));
                        self.timer_g.replace(timer_g);
                    }
                }

                // start Timer D
                let timer_d = self
                    .endpoint_inner
                    .timers
                    .timeout(T1X64, TransactionTimer::TimerD(self.key.clone()));
                self.timer_d.replace(timer_d);
            }
            TransactionState::Confirmed => {
                self.cleanup_timer();
                // start Timer K, wait for ACK
                let timer_k = self
                    .endpoint_inner
                    .timers
                    .timeout(T4, TransactionTimer::TimerK(self.key.clone()));
                self.timer_k.replace(timer_k);
            }
            TransactionState::Terminated => {
                self.cleanup();
                self.tu_sender.send(TransactionEvent::Terminate).ok(); // tell TU to terminate
            }
        }
        trace!("{} transition: {:?} -> {:?}", self.key, self.state, state);
        self.state = state;
        Ok(self.state.clone())
    }

    fn cleanup_timer(&mut self) {
        self.timer_a
            .take()
            .map(|id| self.endpoint_inner.timers.cancel(id));
        self.timer_b
            .take()
            .map(|id| self.endpoint_inner.timers.cancel(id));
        self.timer_d
            .take()
            .map(|id| self.endpoint_inner.timers.cancel(id));
        self.timer_k
            .take()
            .map(|id| self.endpoint_inner.timers.cancel(id));
        self.timer_g
            .take()
            .map(|id| self.endpoint_inner.timers.cancel(id));
    }

    fn cleanup(&mut self) {
        if self.state == TransactionState::Calling {
            return;
        }
        self.cleanup_timer();
        let last_message = {
            match self.transaction_type {
                TransactionType::ClientInvite => {
                    self.last_ack.take().map(|r| SipMessage::Request(r))
                }
                TransactionType::ServerNonInvite => {
                    self.last_response.take().map(|r| SipMessage::Response(r))
                }
                _ => None,
            }
        };
        self.endpoint_inner
            .detach_transaction(&self.key, last_message);
    }
}

impl Drop for Transaction {
    fn drop(&mut self) {
        self.cleanup();
    }
}
