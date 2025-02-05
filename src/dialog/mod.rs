use crate::{Error, Result};
use rsip::{
    prelude::{HeadersExt, UntypedHeader},
    Request,
};

pub mod authenticate;
pub mod client_dialog;
pub mod dialog;
pub mod dialog_layer;
pub mod invitation;
pub mod registration;
pub mod server_dialog;
#[derive(Clone, PartialEq, Eq, Hash, Debug)]
pub struct DialogId {
    pub call_id: String,
    pub from_tag: String,
    pub to_tag: String,
}

impl TryFrom<&Request> for DialogId {
    type Error = crate::Error;

    fn try_from(request: &Request) -> Result<Self> {
        let call_id = request.call_id_header()?.value().to_string();

        let from_tag = match request.from_header()?.tag()? {
            Some(tag) => tag.value().to_string(),
            None => return Err(Error::Error("from tag not found".to_string())),
        };

        let to_tag = match request.to_header()?.tag()? {
            Some(tag) => tag.value().to_string(),
            None => "".to_string(),
        };

        Ok(DialogId {
            call_id,
            from_tag,
            to_tag,
        })
    }
}

impl std::fmt::Display for DialogId {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}-{}-{}", self.call_id, self.from_tag, self.to_tag)
    }
}
