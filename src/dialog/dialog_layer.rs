use super::{dialog_handle::DialogHandle, DialogID};
use crate::transaction::key::TransactionKey;
use std::{
    collections::HashMap,
    sync::{Arc, RwLock},
};

pub struct DialogLayerInner {
    tx_to_dialogs: RwLock<HashMap<TransactionKey, DialogID>>,
    dialogs: RwLock<HashMap<DialogID, DialogHandle>>,
}
pub type DialogLayerInnerRef = Arc<DialogLayerInner>;

pub struct DialogLayer {
    pub inner: DialogLayerInnerRef,
}
