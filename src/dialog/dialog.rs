use super::{
    authenticate::{handle_client_proxy_authenticate, Credential},
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
    headers::{typed, untyped},
    message::request,
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
use tracing::info;

/// DialogState is the state of the dialog
#[derive(Clone)]
pub enum DialogState {
    Calling(rsip::Request),
    Trying,
    Early(rsip::Response),
    Confirmed(rsip::Response),
    Updated(rsip::Response),
    Notify(rsip::Request),
    Info(rsip::Request),
    Terminated(Option<rsip::StatusCode>),
}

pub struct DialogInner {
    pub id: Mutex<DialogId>,
    pub state: Mutex<DialogState>,
    pub local_seq: AtomicU32,
    pub local_uri: String,
    pub remote_seq: AtomicU32,
    pub remote_uri: Mutex<To>,
    pub credential: Option<Credential>,
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
        matches!(self, DialogState::Confirmed(_))
    }
}

impl DialogInner {
    pub fn new(
        endpoint_inner: EndpointInnerRef,
        state_sender: DialogStateSender,
        tx: Transaction,
        credential: Option<Credential>,
    ) -> Result<Self> {
        let initial_request = tx.original.clone();

        let id = DialogId::try_from(&initial_request)?;
        let remote_uri = initial_request.to_header()?.typed()?;

        Ok(Self {
            id: Mutex::new(id),
            local_seq: AtomicU32::new(1),
            local_uri: initial_request.from_header()?.to_string(),
            remote_seq: AtomicU32::new(0),
            remote_uri: Mutex::new(remote_uri),
            credential,
            endpoint_inner,
            state_sender,
            tu_sender: Mutex::new(None),
            state: Mutex::new(DialogState::Calling(initial_request.clone())),
            initial_request,
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
    ) -> rsip::Request {
        let mut headers = headers.unwrap_or_default();
        let cseq_header = CSeq {
            seq: self.increment_local_seq(),
            method,
        };

        headers.push(Header::CallId(
            self.id.lock().unwrap().call_id.clone().into(),
        ));
        headers.push(Header::From(self.local_uri.clone().into()));
        headers.push(Header::To(self.remote_uri.lock().unwrap().clone().into()));
        headers.push(Header::CSeq(cseq_header.into()));

        rsip::Request {
            method,
            uri: self.local_uri.clone().try_into().unwrap(),
            headers: headers.into(),
            body: body.unwrap_or_default(),
            version: rsip::Version::V2,
        }
    }

    pub(super) fn make_response(
        &self,
        request: &Request,
        status: StatusCode,
        headers: Option<Vec<rsip::Header>>,
        body: Option<Vec<u8>>,
    ) -> rsip::Response {
        let mut resp_headers = request.headers.clone();
        resp_headers.unique_push(Header::To(self.remote_uri.lock().unwrap().clone().into()));

        if let Some(headers) = headers {
            for header in headers {
                resp_headers.unique_push(header);
            }
        }

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

        while let Some(msg) = tx.receive().await {
            match msg {
                SipMessage::Response(resp) => match resp.status_code {
                    StatusCode::Trying => {
                        continue;
                    }
                    StatusCode::ProxyAuthenticationRequired => {
                        if let Some(cred) = &self.credential {
                            tx = handle_client_proxy_authenticate(
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
                            self.transition(DialogState::Terminated(Some(resp.status_code)))?;
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
            DialogState::Updated(_) | DialogState::Notify(_) | DialogState::Info(_) => {
                return Ok(());
            }
            _ => {}
        }
        let mut old_state = self.state.lock().unwrap();
        info!("transitioning state: {} -> {}", old_state, state);
        *old_state = state;
        Ok(())
    }

    pub(super) fn update_remote_tag(&self, tag: Tag) {
        let mut remote_uri = self.remote_uri.lock().unwrap().clone();
        remote_uri = remote_uri.with_tag(tag.clone());
        self.id.lock().unwrap().to_tag = tag.to_string();
        *self.remote_uri.lock().unwrap() = remote_uri;
    }
}

impl std::fmt::Display for DialogState {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            DialogState::Calling(_) => write!(f, "Calling"),
            DialogState::Trying => write!(f, "Trying"),
            DialogState::Early(_) => write!(f, "Early"),
            DialogState::Confirmed(_) => write!(f, "Confirmed"),
            DialogState::Updated(_) => write!(f, "Updated"),
            DialogState::Notify(_) => write!(f, "Notify"),
            DialogState::Info(_) => write!(f, "Info"),
            DialogState::Terminated(code) => write!(f, "Terminated {:?}", code),
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
