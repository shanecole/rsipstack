use super::{
    authenticate::Credential,
    client_dialog::ClientInviteDialog,
    dialog::{DialogInner, DialogStateSender},
    dialog_layer::DialogLayer,
};
use crate::{
    dialog::{dialog::Dialog, DialogId},
    transaction::{
        key::{TransactionKey, TransactionRole},
        make_tag,
        transaction::Transaction,
    },
    Result,
};
use rsip::{Request, Response};
use std::sync::Arc;
use tracing::{debug, info};
pub struct InviteOption {
    pub caller: rsip::Uri,
    pub callee: rsip::Uri,
    pub content_type: Option<String>,
    pub offer: Option<Vec<u8>>,
    pub contact: rsip::Uri,
    pub credential: Option<Credential>,
}

impl DialogLayer {
    pub fn make_invite_request(&self, opt: &InviteOption) -> Result<Request> {
        let last_seq = self.increment_last_seq();
        let to = rsip::typed::To {
            display_name: None,
            uri: opt.callee.clone(),
            params: vec![],
        };
        let recipient = to.uri.clone();

        let form = rsip::typed::From {
            display_name: None,
            uri: opt.caller.clone(),
            params: vec![],
        }
        .with_tag(make_tag());

        let via = self.endpoint.get_via(None, None)?;
        let mut request =
            self.endpoint
                .make_request(rsip::Method::Invite, recipient, via, form, to, last_seq);

        let contact = rsip::typed::Contact {
            display_name: None,
            uri: opt.contact.clone(),
            params: vec![],
        };

        request
            .headers
            .unique_push(rsip::Header::Contact(contact.into()));

        request.headers.unique_push(rsip::Header::ContentType(
            opt.content_type
                .clone()
                .unwrap_or("application/sdp".to_string())
                .into(),
        ));
        Ok(request)
    }

    pub async fn do_invite(
        &self,
        opt: InviteOption,
        state_sender: DialogStateSender,
    ) -> Result<(ClientInviteDialog, Option<Response>)> {
        let mut request = self.make_invite_request(&opt)?;
        request.body = opt.offer.unwrap_or_default();
        request.headers.unique_push(rsip::Header::ContentLength(
            (request.body.len() as u32).into(),
        ));

        let id = DialogId::try_from(&request)?;
        let dlg_inner = DialogInner::new(
            TransactionRole::Client,
            id.clone(),
            request.clone(),
            self.endpoint.clone(),
            state_sender,
            opt.credential,
            Some(opt.contact),
        )?;

        let key =
            TransactionKey::from_request(&dlg_inner.initial_request, TransactionRole::Client)?;
        let tx = Transaction::new_client(key, request.clone(), self.endpoint.clone(), None);

        let dialog = ClientInviteDialog {
            inner: Arc::new(dlg_inner),
        };
        self.inner
            .dialogs
            .write()
            .unwrap()
            .insert(id.clone(), Dialog::ClientInvite(dialog.clone()));

        info!("client invite dialog created: {:?}", id);

        match dialog.process_invite(tx).await {
            Ok((new_dialog_id, resp)) => {
                debug!(
                    "client invite dialog confirmed: {} => {}",
                    id, new_dialog_id
                );
                self.inner.dialogs.write().unwrap().remove(&id);
                // update with new dialog id
                self.inner
                    .dialogs
                    .write()
                    .unwrap()
                    .insert(new_dialog_id, Dialog::ClientInvite(dialog.clone()));
                return Ok((dialog, resp));
            }
            Err(e) => {
                self.inner.dialogs.write().unwrap().remove(&id);
                return Err(e);
            }
        }
    }
}
