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
        make_to_tag, random_text,
        transaction::{Transaction, TransactionEventSender},
        TO_TAG_LEN,
    },
    Result,
};
use rsip::{
    headers::{via, Route},
    param::Tag,
    prelude::{HeadersExt, ToTypedHeader},
    typed::{CSeq, To},
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
    pub request_uri: String,
    pub local_seq: AtomicU32,
    pub local_uri: String,
    pub local_contact: Option<rsip::typed::Contact>,

    pub remote_seq: AtomicU32,
    pub remote_uri: To,
    pub remote_contact: Option<rsip::headers::untyped::contact::Contact>,

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
    pub fn new(
        id: DialogId,
        initial_request: Request,
        endpoint_inner: EndpointInnerRef,
        state_sender: DialogStateSender,
        credential: Option<Credential>,
        local_contact: Option<rsip::typed::Contact>,
        remote_contact: Option<rsip::headers::untyped::contact::Contact>,
    ) -> Result<Self> {
        let remote_uri = initial_request.to_header()?.typed()?;
        let local_uri = initial_request.from_header()?.typed()?.to_string();
        let request_uri = initial_request.uri.to_string();

        Ok(Self {
            cancel_token: CancellationToken::new(),
            id: id.clone(),
            local_seq: AtomicU32::new(1),
            request_uri,
            local_uri,
            remote_seq: AtomicU32::new(0),
            remote_uri,
            credential,
            route_set: vec![],
            endpoint_inner,
            state_sender,
            tu_sender: Mutex::new(None),
            state: Mutex::new(DialogState::Calling(id)),
            initial_request,
            local_contact,
            remote_contact,
        })
    }

    pub fn is_confirmed(&self) -> bool {
        self.state.lock().unwrap().is_confirmed()
    }

    pub fn increment_local_seq(&self) -> u32 {
        self.local_seq.fetch_add(1, Ordering::Relaxed)
    }

    pub(super) fn make_request(
        &self,
        method: rsip::Method,
        headers: Option<Vec<rsip::Header>>,
        body: Option<Vec<u8>>,
    ) -> Result<rsip::Request> {
        let mut headers = headers.unwrap_or_default();
        let cseq_header = CSeq {
            seq: self.increment_local_seq(),
            method,
        };

        let via = self.endpoint_inner.get_via()?;
        headers.push(via.into());
        headers.push(Header::CallId(self.id.call_id.clone().into()));
        headers.push(Header::From(self.local_uri.clone().into()));
        headers.push(Header::To(self.remote_uri.clone().into()));
        headers.push(Header::CSeq(cseq_header.into()));

        self.local_contact
            .as_ref()
            .map(|c| headers.push(c.clone().into()));

        for route in &self.route_set {
            headers.push(Header::Route(route.clone()));
        }
        headers.push(Header::MaxForwards(70.into()));

        body.as_ref().map(|b| {
            headers.push(Header::ContentLength((b.len() as u32).into()));
        });

        let req = rsip::Request {
            method,
            uri: self.request_uri.clone().try_into()?,
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
        let mut to = self.remote_uri.clone();
        if status != StatusCode::Trying {
            to = to.with_tag(self.id.to_tag.clone().into());
        }

        let mut resp_headers = rsip::Headers::default();
        resp_headers.push(Header::From(self.local_uri.clone().into()));
        resp_headers.push(Header::To(to.into()));
        resp_headers.push(Header::CallId(self.id.call_id.clone().into()));
        let cseq = request.cseq_header().unwrap();
        resp_headers.push(cseq.clone().into());

        self.local_contact
            .as_ref()
            .map(|c| resp_headers.push(c.clone().into()));

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

#[cfg(test)]
mod tests {
    use crate::transaction::transaction::Transaction;

    use crate::dialog::client_dialog::ClientInviteDialog;

    pub async fn test_dialog() {
        let mut tx = Transaction::new_client(todo!(), todo!(), todo!(), todo!());

        let mut dialog = ClientInviteDialog::new();
        let mut dialog_ref = dialog.clone();
        tokio::spawn(async move {
            dialog_ref.handle_invite(tx).await.ok();
        });
        dialog.bye().await.ok();
    }
}
