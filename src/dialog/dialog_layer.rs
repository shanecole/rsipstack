use super::{
    dialog::{DialogInner, DialogInnerRef},
    server_dialog::ServerInviteDialog,
    DialogId,
};
use crate::transaction::{endpoint::EndpointInnerRef, transaction::Transaction};
use crate::Result;
use std::{
    collections::HashMap,
    sync::{Arc, RwLock},
};

pub struct DialogLayerInner {
    dialogs: RwLock<HashMap<DialogId, DialogInnerRef>>,
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

    pub fn create_or_create_server_invite(&self, tx: Transaction) -> Result<ServerInviteDialog> {
        //let dialog_inner = DialogInner::new(self
        //    , state_sender, tx, credential)

        todo!()
    }

    pub fn get_server_invite_dialog(&self, id: &DialogId) -> Option<ServerInviteDialog> {
        todo!()
    }
}
