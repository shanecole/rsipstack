use std::sync::{Arc, Mutex};

use rsip::{prelude::HeadersExt, Request};

use crate::Error;

pub mod authenticate;
pub mod client_dialog;
pub mod dialog;
pub mod dialog_layer;
pub mod server_dialog;

const TO_TAG_LEN: usize = 8;
const BRANCH_LEN: usize = 12;
const CNONCE_LEN: usize = 8;
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

pub fn make_via_branch() -> rsip::Param {
    rsip::Param::Branch(format!("z9hG4bK{}", random_text(BRANCH_LEN)).into())
}

#[cfg(not(target_family = "wasm"))]
pub fn random_text(count: usize) -> String {
    use rand::Rng;
    rand::thread_rng()
        .sample_iter(rand::distributions::Alphanumeric)
        .take(count)
        .map(char::from)
        .collect::<String>()
        .to_lowercase()
}

#[cfg(target_family = "wasm")]
pub fn random_text(count: usize) -> String {
    (0..count)
        .map(|_| {
            let r = js_sys::Math::random();
            let c = (r * 16.0) as u8;
            if c < 10 {
                (c + 48) as char
            } else {
                (c + 87) as char
            }
        })
        .collect()
}
