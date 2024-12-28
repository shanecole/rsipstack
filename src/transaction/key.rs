use std::hash::Hash;

use crate::{Error, Result};
use rsip::{
    param::Tag,
    prelude::{HeadersExt, ToTypedHeader},
    HostWithPort, Method,
};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Rfc2543 {
    pub method: Method,
    pub cseq: u32,
    pub from_tag: Tag,
    pub call_id: String,
    pub via_host_port: HostWithPort,
}

impl Hash for Rfc2543 {
    fn hash<H: std::hash::Hasher>(&self, state: &mut H) {
        self.method.to_string().hash(state);
        self.cseq.hash(state);
        self.from_tag.to_string().hash(state);
        self.call_id.hash(state);
        self.via_host_port.to_string().hash(state);
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Rfc3261 {
    pub branch: String,
    pub method: Method,
    pub cseq: u32,
    pub from_tag: Tag,
    pub call_id: String,
}

impl Hash for Rfc3261 {
    fn hash<H: std::hash::Hasher>(&self, state: &mut H) {
        self.branch.hash(state);
        self.method.to_string().hash(state);
        self.cseq.hash(state);
        self.from_tag.to_string().hash(state);
        self.call_id.hash(state);
    }
}

#[derive(Clone, PartialEq, Eq, Hash, Debug)]
pub enum TransactionKey {
    RFC3261(Rfc3261),
    RFC2543(Rfc2543),
    Invalid,
}

impl std::fmt::Display for TransactionKey {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            TransactionKey::RFC3261(rfc3261) => write!(
                f,
                "{} {}/{} {}({})",
                rfc3261.call_id, rfc3261.method, rfc3261.cseq, rfc3261.from_tag, rfc3261.branch,
            ),
            TransactionKey::RFC2543(rfc2543) => write!(
                f,
                "{} {}/{} {}[{}]",
                rfc2543.call_id,
                rfc2543.method,
                rfc2543.cseq,
                rfc2543.from_tag,
                rfc2543.via_host_port
            ),
            TransactionKey::Invalid => write!(f, "INVALID"),
        }
    }
}

impl TryFrom<&rsip::Request> for TransactionKey {
    type Error = crate::error::Error;

    fn try_from(req: &rsip::Request) -> Result<Self> {
        let via = req.via_header()?.typed()?;
        match via.branch() {
            Some(branch) => Ok(TransactionKey::RFC3261(Rfc3261 {
                branch: branch.to_string(),
                method: req.method().clone(),
                cseq: req.cseq_header()?.seq()?,
                from_tag: req.from_header()?.tag()?.ok_or(Error::TransactionError(
                    "from tags missing".to_string(),
                    TransactionKey::Invalid,
                ))?,
                call_id: req.call_id_header()?.to_string(),
            })),
            None => Ok(TransactionKey::RFC2543(Rfc2543 {
                method: req.method().clone(),
                cseq: req.cseq_header()?.seq()?,
                from_tag: req.from_header()?.tag()?.ok_or(Error::TransactionError(
                    "from tags missing".to_string(),
                    TransactionKey::Invalid,
                ))?,
                call_id: req.call_id_header()?.to_string(),
                via_host_port: via.uri.host_with_port,
            })),
        }
    }
}

impl TryFrom<&rsip::Response> for TransactionKey {
    type Error = crate::error::Error;

    fn try_from(resp: &rsip::Response) -> Result<Self> {
        let via = resp.via_header()?.typed()?;
        let cseq = resp.cseq_header()?;
        match via.branch() {
            Some(branch) => Ok(TransactionKey::RFC3261(Rfc3261 {
                branch: branch.to_string(),
                method: cseq.method()?,
                cseq: cseq.seq()?,
                from_tag: resp.from_header()?.tag()?.ok_or(Error::TransactionError(
                    "from tags missing".to_string(),
                    TransactionKey::Invalid,
                ))?,
                call_id: resp.call_id_header()?.to_string(),
            })),
            None => Ok(TransactionKey::RFC2543(Rfc2543 {
                method: cseq.method()?,
                cseq: cseq.seq()?,
                from_tag: resp.from_header()?.tag()?.ok_or(Error::TransactionError(
                    "from tags missing".to_string(),
                    TransactionKey::Invalid,
                ))?,
                call_id: resp.call_id_header()?.to_string(),
                via_host_port: via.uri.host_with_port,
            })),
        }
    }
}

#[test]
fn test_transaction_key() -> Result<()> {
    use rsip::headers::*;
    let register_req = rsip::message::Request {
        method: rsip::method::Method::Register,
        uri: rsip::Uri {
            scheme: Some(rsip::Scheme::Sips),
            host_with_port: rsip::Domain::from("example.com").into(),
            ..Default::default()
        },
        headers: vec![
            Via::new("SIP/2.0/TLS client.biloxi.example.com:5061;branch=z9hG4bKnashd92").into(),
            CSeq::new("2 REGISTER").into(),
            From::new("Bob <sips:bob@biloxi.example.com>;tag=ja743ks76zlflH").into(),
            CallId::new("1j9FpLxk3uxtm8tn@biloxi.example.com").into(),
        ]
        .into(),
        version: rsip::Version::V2,
        body: Default::default(),
    };
    _ = TransactionKey::try_from(&register_req)?;
    Ok(())
}
