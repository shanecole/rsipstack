use super::{
    authenticate::{handle_client_authenticate, Credential},
    client_dialog::ClientInviteDialog,
    server_dialog::ServerInviteDialog,
    DialogId,
};
use crate::{
    transaction::{
        endpoint::EndpointInnerRef,
        key::{TransactionKey, TransactionRole},
        transaction::{Transaction, TransactionEventSender},
    },
    Result,
};
use rsip::{
    headers::Route,
    prelude::{HeadersExt, ToTypedHeader, UntypedHeader},
    typed::{CSeq, Contact, From, To},
    Header, Request, Response, SipMessage, StatusCode,
};
use std::sync::{
    atomic::{AtomicU32, Ordering},
    Arc, Mutex,
};
use tokio::sync::mpsc::{UnboundedReceiver, UnboundedSender};
use tokio_util::sync::CancellationToken;
use tracing::info;

/// DialogState is the state of the dialog
#[derive(Clone)]
pub enum DialogState {
    Calling(DialogId),
    Trying(DialogId),
    Early(DialogId, rsip::Response),
    Confirmed(DialogId, rsip::Response),
    Updated(DialogId, rsip::Request),
    Notify(DialogId, rsip::Request),
    Info(DialogId, rsip::Request),
    Terminated(DialogId, Option<rsip::StatusCode>),
}
#[derive(Clone)]
pub enum Dialog {
    ServerInvite(ServerInviteDialog),
    ClientInvite(ClientInviteDialog),
}

pub struct DialogInner {
    pub cancel_token: CancellationToken,
    pub id: DialogId,
    pub state: Mutex<DialogState>,

    pub local_seq: AtomicU32,
    pub local_contact: Option<rsip::Uri>,

    pub remote_seq: AtomicU32,
    pub remote_uri: rsip::Uri,
    pub from: From,
    pub to: To,

    pub credential: Option<Credential>,
    pub route_set: Vec<Route>,
    pub(super) endpoint_inner: EndpointInnerRef,
    pub(super) state_sender: DialogStateSender,
    pub(super) tu_sender: TuSenderRef,
    pub(super) initial_request: Request,
}

pub type DialogStateReceiver = UnboundedReceiver<DialogState>;
pub type DialogStateSender = UnboundedSender<DialogState>;

pub(super) type DialogInnerRef = Arc<DialogInner>;
pub(super) type TuSenderRef = Mutex<Option<TransactionEventSender>>;

impl DialogState {
    pub fn is_confirmed(&self) -> bool {
        matches!(self, DialogState::Confirmed(_, _))
    }
}

impl DialogInner {
    pub fn new_server(
        id: DialogId,
        initial_request: Request,
        endpoint_inner: EndpointInnerRef,
        state_sender: DialogStateSender,
        credential: Option<Credential>,
        local_contact: Option<rsip::Uri>,
    ) -> Result<Self> {
        let cseq = initial_request.cseq_header()?.seq()?;
        let from = initial_request.from_header()?.typed()?.clone();
        let to = initial_request.to_header()?.typed()?;

        let remote_contact = initial_request.contact_header()?;
        //TODO: fix this hack
        let line = remote_contact.value().replace("\"lime\"", "lime");
        let mut remote_uri = rsip::headers::untyped::Contact::try_from(line.as_str())
            .map_err(|e| {
                info!("error parsing contact header {}", e);
                crate::Error::DialogError(e.to_string(), id.clone())
            })?
            .typed()?
            .uri;
        remote_uri.params = vec![];

        Ok(Self {
            cancel_token: CancellationToken::new(),
            id: id.clone(),
            local_seq: AtomicU32::new(cseq),
            remote_uri,
            remote_seq: AtomicU32::new(cseq),
            from,
            to,
            credential,
            route_set: vec![],
            endpoint_inner,
            state_sender,
            tu_sender: Mutex::new(None),
            state: Mutex::new(DialogState::Calling(id)),
            initial_request,
            local_contact,
        })
    }

    pub fn is_confirmed(&self) -> bool {
        self.state.lock().unwrap().is_confirmed()
    }

    pub fn increment_local_seq(&self) -> u32 {
        self.local_seq.fetch_add(1, Ordering::Relaxed);
        self.local_seq.load(Ordering::Relaxed)
    }

    pub fn increment_remove_seq(&self) -> u32 {
        self.remote_seq.fetch_add(1, Ordering::Relaxed);
        self.remote_seq.load(Ordering::Relaxed)
    }

    pub(super) fn make_request(
        &self,
        method: rsip::Method,
        headers: Option<Vec<rsip::Header>>,
        body: Option<Vec<u8>>,
    ) -> Result<rsip::Request> {
        let mut headers = headers.unwrap_or_default();
        let cseq_header = CSeq {
            seq: self.increment_remove_seq(),
            method,
        };

        let to = self.to.clone().with_tag(self.id.to_tag.clone().into());
        let via = self.endpoint_inner.get_via()?;

        headers.push(via.into());
        headers.push(Header::CallId(self.id.call_id.clone().into()));
        headers.push(Header::From(to.to_string().into()));
        headers.push(Header::To(self.from.to_string().into()));

        headers.push(Header::CSeq(cseq_header.into()));

        self.local_contact
            .as_ref()
            .map(|c| headers.push(Contact::from(c.clone()).into()));

        for route in &self.route_set {
            headers.push(Header::Route(route.clone()));
        }
        headers.push(Header::MaxForwards(70.into()));

        body.as_ref().map(|b| {
            headers.push(Header::ContentLength((b.len() as u32).into()));
        });

        let req = rsip::Request {
            method,
            uri: self.remote_uri.clone(),
            headers: headers.into(),
            body: body.unwrap_or_default(),
            version: rsip::Version::V2,
        };
        Ok(req)
    }

    pub(super) fn make_response(
        &self,
        request: &Request,
        status: StatusCode,
        headers: Option<Vec<rsip::Header>>,
        body: Option<Vec<u8>>,
    ) -> rsip::Response {
        let mut to = self.to.clone();
        if status != StatusCode::Trying {
            to = to.with_tag(self.id.to_tag.clone().into());
        }

        let mut resp_headers = rsip::Headers::default();
        resp_headers.push(Header::From(self.from.clone().into()));
        resp_headers.push(Header::To(to.into()));
        resp_headers.push(Header::CallId(self.id.call_id.clone().into()));
        let cseq = request.cseq_header().unwrap();
        resp_headers.push(cseq.clone().into());

        self.local_contact
            .as_ref()
            .map(|c| resp_headers.push(Contact::from(c.clone()).into()));

        for header in request.headers.iter() {
            match header {
                Header::RecordRoute(route) => {
                    resp_headers.push(Header::RecordRoute(route.clone()));
                }
                Header::Via(via) => {
                    resp_headers.push(Header::Via(via.clone()));
                }
                _ => {}
            }
        }

        if let Some(headers) = headers {
            for header in headers {
                resp_headers.unique_push(header);
            }
        }

        body.as_ref().map(|b| {
            resp_headers.push(Header::ContentLength((b.len() as u32).into()));
        });

        Response {
            status_code: status,
            headers: resp_headers,
            body: body.unwrap_or_default(),
            version: request.version().clone(),
        }
    }

    pub(super) async fn do_request(&self, request: &Request) -> Result<Option<rsip::Response>> {
        let key = TransactionKey::from_request(request, TransactionRole::Client)?;
        let mut tx =
            Transaction::new_client(key, request.clone(), self.endpoint_inner.clone(), None);
        tx.send().await?;
        let mut auth_sent = false;

        while let Some(msg) = tx.receive().await {
            match msg {
                SipMessage::Response(resp) => match resp.status_code {
                    StatusCode::Trying => {
                        continue;
                    }
                    StatusCode::ProxyAuthenticationRequired | StatusCode::Unauthorized => {
                        let id = self.id.clone();
                        if auth_sent {
                            info!("received {} response after auth sent", resp.status_code);
                            self.transition(DialogState::Terminated(id, Some(resp.status_code)))?;
                            break;
                        }
                        auth_sent = true;
                        if let Some(cred) = &self.credential {
                            tx = handle_client_authenticate(
                                self.increment_local_seq(),
                                tx,
                                resp,
                                cred,
                            )
                            .await?;
                            tx.send().await?;
                            continue;
                        } else {
                            info!("received 407 response without auth option");
                            self.transition(DialogState::Terminated(id, Some(resp.status_code)))?;
                        }
                    }
                    _ => {
                        info!("dialog do_request done: {:?}", resp.status_code);
                        return Ok(Some(resp));
                    }
                },
                _ => break,
            }
        }
        Ok(None)
    }

    pub(super) fn transition(&self, state: DialogState) -> Result<()> {
        self.state_sender.send(state.clone())?;
        match state {
            DialogState::Updated(_, _) | DialogState::Notify(_, _) | DialogState::Info(_, _) => {
                return Ok(());
            }
            _ => {}
        }
        let mut old_state = self.state.lock().unwrap();
        info!("transitioning state: {} -> {}", old_state, state);
        *old_state = state;
        Ok(())
    }
}

impl std::fmt::Display for DialogState {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            DialogState::Calling(id) => write!(f, "{}(Calling)", id),
            DialogState::Trying(id) => write!(f, "{}(Trying)", id),
            DialogState::Early(id, _) => write!(f, "{}(Early)", id),
            DialogState::Confirmed(id, _) => write!(f, "{}(Confirmed)", id),
            DialogState::Updated(id, _) => write!(f, "{}(Updated)", id),
            DialogState::Notify(id, _) => write!(f, "{}(Notify)", id),
            DialogState::Info(id, _) => write!(f, "{}(Info)", id),
            DialogState::Terminated(id, code) => write!(f, "{}(Terminated {:?})", id, code),
        }
    }
}

impl Dialog {
    pub fn id(&self) -> DialogId {
        match self {
            Dialog::ServerInvite(d) => d.inner.id.clone(),
            Dialog::ClientInvite(d) => d.inner.id.clone(),
        }
    }
}
