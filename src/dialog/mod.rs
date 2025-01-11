use crate::Error;
use rsip::{prelude::HeadersExt, Request};

pub mod authenticate;
pub mod client_dialog;
pub mod dialog;
pub mod dialog_layer;
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

    fn try_from(request: &Request) -> Result<Self, Self::Error> {
        let call_id = request.call_id_header()?.to_string();

        let from_tag = request
            .from_header()?
            .tag()?
            .ok_or(Error::Error("from tags missing".to_string()))?;

        let to_tag = request.to_header()?.tag()?.unwrap_or_default();

        Ok(DialogId {
            call_id: call_id.to_string(),
            from_tag: from_tag.to_string(),
            to_tag: to_tag.to_string(),
        })
    }
}

impl std::fmt::Display for DialogId {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}-{}-{}", self.call_id, self.from_tag, self.to_tag)
    }
}
