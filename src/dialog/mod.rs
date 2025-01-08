use crate::transaction::endpoint::EndpointInnerRef;
use dialog_layer::{DialogLayerInnerRef, DialogMessageReceiver};
use std::sync::atomic::AtomicU32;
use tokio::sync::mpsc::{UnboundedReceiver, UnboundedSender};

pub mod dialog;
pub mod dialog_layer;

#[derive(Clone, PartialEq, Eq, Hash, Debug)]
pub struct DialogID {
    pub call_id: String,
    pub from_tag: String,
    pub to_tag: String,
}

pub trait DialogMedia {}

pub enum DialogState {
    Calling,
    Trying,
    Early,
    Confirmed,
    Terminated,
}

pub type DialogStateReceiver = UnboundedReceiver<DialogState>;
pub type DialogStateSender = UnboundedSender<DialogState>;

pub struct DialogInner {
    pub id: DialogID,
    pub state: DialogState,
    pub local_seq: AtomicU32,
    pub local_tag: String,
    pub local_uri: rsip::Uri,
    pub remote_uri: Option<rsip::Uri>,
    pub remote_tag: Option<String>,
    pub remote_seq: AtomicU32,
    pub route_set: Vec<rsip::typed::Route>,
    pub secure: bool,
    pub incoming: DialogMessageReceiver,
    endpoint_inner: EndpointInnerRef,
    dialog_layer_inner: DialogLayerInnerRef,
    state_sender: DialogStateSender,
}

impl Drop for DialogInner {
    fn drop(&mut self) {}
}
