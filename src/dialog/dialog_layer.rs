use super::authenticate::Credential;
use super::dialog::DialogStateSender;
use super::{dialog::Dialog, server_dialog::ServerInviteDialog, DialogId};
use crate::dialog::dialog::DialogInner;
use crate::transaction::key::TransactionRole;
use crate::transaction::make_tag;
use crate::transaction::{endpoint::EndpointInnerRef, transaction::Transaction};
use crate::{dialog, Result};
use rsip::Request;
use std::sync::atomic::{AtomicU32, Ordering};
use std::{
    collections::HashMap,
    sync::{Arc, RwLock},
};
use tracing::info;

pub struct DialogLayerInner {
    pub(super) last_seq: AtomicU32,
    pub(super) dialogs: RwLock<HashMap<DialogId, Dialog>>,
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
                last_seq: AtomicU32::new(0),
                dialogs: RwLock::new(HashMap::new()),
            }),
        }
    }

    pub fn get_or_create_server_invite(
        &self,
        tx: &Transaction,
        state_sender: DialogStateSender,
        credential: Option<Credential>,
        contact: Option<rsip::Uri>,
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
        id.to_tag = make_tag().to_string(); // generate to tag

        let dlg_inner = DialogInner::new(
            TransactionRole::Server,
            id.clone(),
            tx.original.clone(),
            self.endpoint.clone(),
            state_sender,
            credential,
            contact,
        )?;

        let dialog = ServerInviteDialog {
            inner: Arc::new(dlg_inner),
        };
        self.inner
            .dialogs
            .write()
            .unwrap()
            .insert(id.clone(), Dialog::ServerInvite(dialog.clone()));
        info!("server invite dialog created: {:?}", id);
        Ok(dialog)
    }

    pub fn increment_last_seq(&self) -> u32 {
        self.inner.last_seq.fetch_add(1, Ordering::Relaxed);
        self.inner.last_seq.load(Ordering::Relaxed)
    }

    pub fn get_dialog(&self, id: &DialogId) -> Option<Dialog> {
        let dialogs = self.inner.dialogs.read().unwrap();
        match dialogs.get(id) {
            Some(dialog) => return Some(dialog.clone()),
            None => {}
        }
        let swap_id = DialogId {
            call_id: id.call_id.clone(),
            from_tag: id.to_tag.clone(),
            to_tag: id.from_tag.clone(),
        };
        match dialogs.get(&swap_id) {
            Some(dialog) => Some(dialog.clone()),
            None => None,
        }
    }

    pub fn remove_dialog(&self, id: &DialogId) {
        info!("remove dialog: {:?}", id);
        self.inner
            .dialogs
            .write()
            .unwrap()
            .remove(id)
            .map(|d| d.on_remove());
    }

    pub fn match_dialog(&self, req: &Request) -> Option<Dialog> {
        let id = DialogId::try_from(req).ok()?;
        self.get_dialog(&id)
    }
}
