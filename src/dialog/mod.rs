pub mod dialog;
//pub mod dialog_example;
pub mod dialog_handle;
pub mod dialog_layer;

#[derive(Clone, PartialEq, Eq, Hash, Debug)]
pub struct DialogID {
    pub call_id: String,
    pub from_tag: String,
    pub to_tag: String,
}
