use super::{
    key::TransactionKey,
    make_via_branch,
    timer::Timer,
    transaction::{Transaction, TransactionEvent, TransactionEventSender},
    SipConnection, TransactionReceiver, TransactionSender, TransactionTimer,
};
use crate::{
    transport::{SipAddr, TransportEvent, TransportLayer},
    Error, Result, USER_AGENT,
};
use rsip::SipMessage;
use std::{
    collections::HashMap,
    sync::{Arc, Mutex},
    time::{Duration, Instant},
};
use tokio::sync::mpsc::{UnboundedReceiver, UnboundedSender};
use tokio::{
    select,
    sync::mpsc::{error, unbounded_channel},
    time::sleep,
};
use tokio_util::sync::CancellationToken;
use tracing::{debug, info, trace, warn};

pub struct EndpointInner {
    pub user_agent: String,
    pub timers: Timer<TransactionTimer>,
    pub transport_layer: TransportLayer,
    pub finished_transactions: Mutex<HashMap<TransactionKey, Option<SipMessage>>>,
    pub transactions: Mutex<HashMap<TransactionKey, TransactionEventSender>>,
    incoming_sender: Mutex<Option<TransactionSender>>,
    cancel_token: CancellationToken,
    timer_interval: Duration,
    pub transport_tx: UnboundedSender<TransportEvent>,
    transport_rx: Mutex<UnboundedReceiver<TransportEvent>>,

    pub t1: Duration,
    pub t4: Duration,
    pub t1x64: Duration,
}
pub type EndpointInnerRef = Arc<EndpointInner>;

pub struct EndpointBuilder {
    user_agent: String,
    transport_layer: Option<TransportLayer>,
    cancel_token: Option<CancellationToken>,
    timer_interval: Option<Duration>,
}

pub struct Endpoint {
    pub inner: EndpointInnerRef,
}

impl EndpointInner {
    pub fn new(
        user_agent: String,
        transport_layer: TransportLayer,
        cancel_token: CancellationToken,
        timer_interval: Option<Duration>,
    ) -> Arc<Self> {
        let (transport_tx, transport_rx) = unbounded_channel();
        Arc::new(EndpointInner {
            user_agent,
            timers: Timer::new(),
            transport_layer,
            transactions: Mutex::new(HashMap::new()),
            finished_transactions: Mutex::new(HashMap::new()),
            timer_interval: timer_interval.unwrap_or(Duration::from_millis(20)),
            transport_tx,
            transport_rx: Mutex::new(transport_rx),
            cancel_token,
            incoming_sender: Mutex::new(None),
            t1: Duration::from_millis(500),
            t4: Duration::from_secs(4),
            t1x64: Duration::from_millis(64 * 500),
        })
    }

    pub async fn serve(self: &Arc<Self>) -> Result<()> {
        select! {
            _ = self.cancel_token.cancelled() => {
            },
            r = self.clone().process_timer() => {
                _ = r?
            },
            r = self.clone().process_transport_layer() => {
                _ = r?
            },
        }
        Ok(())
    }

    // process transport layer, receive message from transport layer
    async fn process_transport_layer(self: Arc<Self>) -> Result<()> {
        let transport_tx = self.transport_tx.clone();
        self.transport_layer.serve_listens(transport_tx).await.ok();

        let mut transport_rx = self.transport_rx.lock().unwrap();
        while let Some(event) = transport_rx.recv().await {
            match event {
                TransportEvent::Incoming(msg, connection, from) => {
                    match self.on_received_message(msg, connection).await {
                        Ok(()) => {}
                        Err(e) => {
                            warn!("on_received_message error:{} {:?}", from, e);
                        }
                    }
                }
                TransportEvent::New(t) => {
                    trace!("new connection {} ", t);
                }
                TransportEvent::Closed(t) => {
                    trace!("connection closed {} ", t);
                }
            }
        }
        Ok(())
    }

    pub async fn process_timer(self: Arc<Self>) -> Result<()> {
        while !self.cancel_token.is_cancelled() {
            for t in self.timers.poll(Instant::now()) {
                match t {
                    TransactionTimer::TimerCleanup(key) => {
                        debug!("TimerCleanup {}", key);
                        self.transactions.lock().unwrap().remove(&key);
                        self.finished_transactions.lock().unwrap().remove(&key);
                        continue;
                    }
                    _ => {}
                }

                if let Some(tu) = { self.transactions.lock().unwrap().get(t.key()) } {
                    match tu.send(TransactionEvent::Timer(t)) {
                        Ok(_) => {}
                        Err(error::SendError(t)) => match t {
                            TransactionEvent::Timer(t) => {
                                self.detach_transaction(t.key(), None);
                            }
                            _ => {}
                        },
                    }
                }
            }
            sleep(self.timer_interval).await;
        }
        Ok(())
    }

    pub fn attach_incoming_sender(&self, sender: Option<TransactionSender>) {
        *self.incoming_sender.lock().unwrap() = sender;
    }

    // receive message from transport layer
    pub async fn on_received_message(
        self: &Arc<Self>,
        msg: SipMessage,
        connection: SipConnection,
    ) -> Result<()> {
        let mut key = match &msg {
            SipMessage::Request(req) => {
                TransactionKey::from_request(req, super::key::TransactionRole::Server)?
            }
            SipMessage::Response(resp) => {
                TransactionKey::from_response(resp, super::key::TransactionRole::Client)?
            }
        };

        // check is the termination of an existing transaction
        let last_message = self
            .finished_transactions
            .lock()
            .unwrap()
            .get(&key)
            .map(|m| m.clone())
            .flatten();

        if let Some(last_message) = last_message {
            connection.send(last_message, None).await?;
            return Ok(());
        }

        match self.transactions.lock().unwrap().get(&key) {
            Some(tu) => {
                tu.send(TransactionEvent::Received(msg, Some(connection)))
                    .map_err(|e| Error::TransactionError(e.to_string(), key))?;
                return Ok(());
            }
            None => {}
        }
        // if the transaction is not exist, create a new transaction
        let request = match msg {
            SipMessage::Request(req) => req,
            SipMessage::Response(resp) => {
                debug!("the transaction is not exist {} {}", key, resp);
                return Ok(());
            }
        };

        if self.incoming_sender.lock().unwrap().is_none() {
            let resp = self.make_response(&request, rsip::StatusCode::ServiceUnavailable, None);
            connection.send(resp.into(), None).await?;
            return Err(Error::TransactionError(
                "incoming_sender not set".to_string(),
                key,
            ));
        }

        if matches!(request.method, rsip::Method::Ack | rsip::Method::Cancel) {
            key =
                TransactionKey::from_ack_or_cancel(&request, super::key::TransactionRole::Server)?;
        }

        let tx =
            Transaction::new_server(key.clone(), request.clone(), self.clone(), Some(connection));

        self.incoming_sender
            .lock()
            .unwrap()
            .as_ref()
            .map(|s| s.send(tx));
        return Ok(());
    }

    pub fn attach_transaction(&self, key: &TransactionKey, tu_sender: TransactionEventSender) {
        trace!("attach_transaction {}", key);
        self.transactions
            .lock()
            .unwrap()
            .insert(key.clone(), tu_sender);
    }

    pub fn detach_transaction(&self, key: &TransactionKey, last_message: Option<SipMessage>) {
        trace!("detach_transaction {}", key);
        self.transactions.lock().unwrap().remove(key);

        if let Some(msg) = last_message {
            let timer_k_duration = if msg.is_request() {
                self.t4
            } else {
                self.t1x64
            };

            self.timers.timeout(
                timer_k_duration,
                TransactionTimer::TimerCleanup(key.clone()), // maybe use TimerK ???
            );

            self.finished_transactions
                .lock()
                .unwrap()
                .insert(key.clone(), Some(msg));
        }
    }

    pub fn get_addrs(&self) -> Vec<SipAddr> {
        self.transport_layer.get_addrs()
    }

    pub fn get_record_route(&self) -> Result<rsip::typed::RecordRoute> {
        let first_addr = self
            .transport_layer
            .get_addrs()
            .first()
            .ok_or(Error::EndpointError("not sipaddrs".to_string()))
            .cloned()?;
        let rr = rsip::UriWithParamsList(vec![rsip::UriWithParams {
            uri: first_addr.into(),
            params: vec![rsip::Param::Other("lr".into(), None)].into(),
        }]);
        Ok(rr.into())
    }

    pub fn get_via(
        &self,
        addr: Option<crate::transport::SipAddr>,
        branch: Option<rsip::Param>,
    ) -> Result<rsip::typed::Via> {
        let first_addr = match addr {
            Some(addr) => addr,
            None => self
                .transport_layer
                .get_addrs()
                .first()
                .ok_or(Error::EndpointError("not sipaddrs".to_string()))
                .cloned()?,
        };

        let via = rsip::typed::Via {
            version: rsip::Version::V2,
            transport: first_addr.r#type.unwrap_or_default(),
            uri: first_addr.addr.into(),
            params: vec![
                branch.unwrap_or_else(|| make_via_branch()),
                rsip::Param::Other("rport".into(), None),
            ]
            .into(),
        };
        Ok(via)
    }
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
        let cancel_token = self.cancel_token.take().unwrap_or_default();

        let transport_layer = self
            .transport_layer
            .take()
            .unwrap_or(TransportLayer::new(cancel_token.child_token()));

        let core = EndpointInner::new(
            self.user_agent.clone(),
            transport_layer,
            cancel_token,
            self.timer_interval,
        );

        Endpoint { inner: core }
    }
}

impl Endpoint {
    pub async fn serve(&self) {
        let inner = self.inner.clone();
        match inner.serve().await {
            Ok(()) => {
                info!("endpoint shutdown");
            }
            Err(e) => {
                warn!("endpoint serve error: {:?}", e);
            }
        }
    }

    pub fn shutdown(&self) {
        info!("endpoint shutdown requested");
        self.inner.cancel_token.cancel();
    }

    //
    // get incoming requests from the endpoint
    //
    pub fn incoming_transactions(&self) -> TransactionReceiver {
        let (tx, rx) = unbounded_channel();
        self.inner.attach_incoming_sender(Some(tx));
        rx
    }

    pub fn get_addrs(&self) -> Vec<SipAddr> {
        self.inner.transport_layer.get_addrs()
    }
}
