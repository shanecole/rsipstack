use crate::transaction::key::TransactionKey;
use rsip::SipMessage;
use std::{
    collections::HashMap,
    sync::{Arc, RwLock},
};
use tokio::sync::mpsc::{UnboundedReceiver, UnboundedSender};

use super::DialogID;

pub type DialogMessageReceiver = UnboundedReceiver<SipMessage>;
pub type DialogMessageSender = UnboundedSender<SipMessage>;

pub struct DialogLayerInner {
    pub tx_to_dialogs: RwLock<HashMap<TransactionKey, DialogID>>,
    pub dialogs: RwLock<HashMap<DialogID, DialogMessageSender>>,
}
pub type DialogLayerInnerRef = Arc<DialogLayerInner>;

pub struct DialogLayer {
    pub inner: DialogLayerInnerRef,
}
