use super::{dialog::Dialog, server_dialog::ServerInviteDialog, DialogId};
use crate::dialog::dialog::DialogInner;
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

    pub fn get_or_create_server_invite(&self, tx: &Transaction) -> Result<ServerInviteDialog> {
        let id = DialogId::try_from(&tx.original)?;
        if !id.to_tag.is_empty() {
            let dlg = self.inner.dialogs.read().unwrap().get(&id).cloned();
            match dlg {
                Some(Dialog::ServerInvite(dlg)) => return Ok(dlg),
                _ => {}
            }
        }

        let dlg_inner = DialogInner::new(self.endpoint.clone(), self.inner.clone(), None)?;
        todo!()
    }

    pub fn get_dialog(&self, id: &DialogId) -> Option<Dialog> {
        self.inner.dialogs.read().unwrap().get(id).cloned()
    }
}
