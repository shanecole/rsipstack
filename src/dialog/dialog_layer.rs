use log::debug;
use rsip::prelude::{HeadersExt, ToTypedHeader};
use tracing_subscriber::field::debug;

use super::authenticate::Credential;
use super::dialog::DialogStateSender;
use super::{dialog::Dialog, server_dialog::ServerInviteDialog, DialogId};
use crate::dialog::dialog::DialogInner;
use crate::transaction::make_to_tag;
use crate::transaction::{endpoint::EndpointInnerRef, transaction::Transaction};
use crate::Result;
use std::{
    collections::HashMap,
    sync::{Arc, RwLock},
};

pub struct DialogLayerInner {
    dialogs: RwLock<HashMap<DialogId, Dialog>>,
}
pub type DialogLayerInnerRef = Arc<DialogLayerInner>;

pub struct DialogLayer {
    pub endpoint: EndpointInnerRef,
    pub inner: DialogLayerInnerRef,
}

impl DialogLayer {
    pub fn new(endpoint: EndpointInnerRef) -> Self {
        Self {
            endpoint,
            inner: Arc::new(DialogLayerInner {
                dialogs: RwLock::new(HashMap::new()),
            }),
        }
    }

    pub fn get_or_create_server_invite(
        &self,
        tx: &Transaction,
        state_sender: DialogStateSender,
        credential: Option<Credential>,
        contact: Option<rsip::typed::Contact>,
    ) -> Result<ServerInviteDialog> {
        let mut id = DialogId::try_from(&tx.original)?;
        if !id.to_tag.is_empty() {
            let dlg = self.inner.dialogs.read().unwrap().get(&id).cloned();
            match dlg {
                Some(Dialog::ServerInvite(dlg)) => return Ok(dlg),
                _ => {
                    return Err(crate::Error::DialogError(
                        "the dialog not found".to_string(),
                        id,
                    ));
                }
            }
        }
        id.to_tag = make_to_tag().to_string(); // generate to tag
        let remote_contact = tx.original.contact_header()?.clone();

        let dlg_inner = DialogInner::new(
            id.clone(),
            tx.original.clone(),
            self.endpoint.clone(),
            state_sender,
            credential,
            contact,
            Some(remote_contact),
        )?;

        let dialog = ServerInviteDialog {
            inner: Arc::new(dlg_inner),
        };
        self.inner
            .dialogs
            .write()
            .unwrap()
            .insert(id.clone(), Dialog::ServerInvite(dialog.clone()));
        debug!("add server dialog: {:?}", id);
        Ok(dialog)
    }

    pub fn get_dialog(&self, id: &DialogId) -> Option<Dialog> {
        self.inner.dialogs.read().unwrap().get(id).cloned()
    }

    pub fn remove_dialog(&self, id: &DialogId) {
        debug!("remove dialog: {:?}", id);
        self.inner.dialogs.write().unwrap().remove(id);
    }
}
