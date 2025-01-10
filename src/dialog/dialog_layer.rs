use super::{dialog::DialogInnerRef, DialogId};
use std::{
    collections::HashMap,
    sync::{Arc, RwLock},
};

pub struct DialogLayerInner {
    dialogs: RwLock<HashMap<DialogId, DialogInnerRef>>,
}
pub type DialogLayerInnerRef = Arc<DialogLayerInner>;

pub struct DialogLayer {
    pub inner: DialogLayerInnerRef,
}
