use crate::{Error, Result};
use rsip::headers::UntypedHeader;
use rsip::{
    param::Tag,
    prelude::{HeadersExt, ToTypedHeader},
    HostWithPort, Method,
};
use std::hash::Hash;

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
        let mut method = req.method().clone();
        if method == Method::Ack {
            method = Method::Invite;
        }
        let from_tag = req.from_header()?.tag()?.ok_or(Error::TransactionError(
            "from tags missing".to_string(),
            TransactionKey::Invalid,
        ))?;
        let call_id = req.call_id_header()?.value().to_string();
        match via.branch() {
            Some(branch) => Ok(TransactionKey::RFC3261(Rfc3261 {
                branch: branch.to_string(),
                method,
                cseq: req.cseq_header()?.seq()?,
                from_tag,
                call_id,
            })),
            None => Ok(TransactionKey::RFC2543(Rfc2543 {
                method,
                cseq: req.cseq_header()?.seq()?,
                from_tag,
                call_id,
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
        let method = cseq.method()?;
        let from_tag = resp.from_header()?.tag()?.ok_or(Error::TransactionError(
            "from tags missing".to_string(),
            TransactionKey::Invalid,
        ))?;
        let call_id = resp.call_id_header()?.value().to_string();
        match via.branch() {
            Some(branch) => Ok(TransactionKey::RFC3261(Rfc3261 {
                branch: branch.to_string(),
                method,
                cseq: cseq.seq()?,
                from_tag,
                call_id,
            })),
            None => Ok(TransactionKey::RFC2543(Rfc2543 {
                method,
                cseq: cseq.seq()?,
                from_tag,
                call_id,
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
    let key = TransactionKey::try_from(&register_req)?;
    assert_eq!(
        key,
        TransactionKey::RFC3261(Rfc3261 {
            branch: "z9hG4bKnashd92".to_string(),
            method: Method::Register,
            cseq: 2,
            from_tag: Tag::new("ja743ks76zlflH"),
            call_id: "1j9FpLxk3uxtm8tn@biloxi.example.com".to_string(),
        })
    );
    let register_resp = rsip::message::Response {
        status_code: rsip::StatusCode::OK,
        version: rsip::Version::V2,
        headers: vec![
            Via::new("SIP/2.0/TLS client.biloxi.example.com:5061;branch=z9hG4bKnashd92").into(),
            CSeq::new("2 REGISTER").into(),
            From::new("Bob <sips:bob@biloxi.example.com>;tag=ja743ks76zlflH").into(),
            CallId::new("1j9FpLxk3uxtm8tn@biloxi.example.com").into(),
        ]
        .into(),
        body: Default::default(),
    };
    let key = TransactionKey::try_from(&register_resp)?;
    assert_eq!(
        key,
        TransactionKey::RFC3261(Rfc3261 {
            branch: "z9hG4bKnashd92".to_string(),
            method: Method::Register,
            cseq: 2,
            from_tag: Tag::new("ja743ks76zlflH"),
            call_id: "1j9FpLxk3uxtm8tn@biloxi.example.com".to_string(),
        })
    );

    let mut ack_req = register_req.clone();
    ack_req.method = Method::Ack;
    ack_req.headers.unique_push(CSeq::new("2 ACK").into());

    let key = TransactionKey::try_from(&ack_req)?;
    assert_eq!(
        key,
        TransactionKey::RFC3261(Rfc3261 {
            branch: "z9hG4bKnashd92".to_string(),
            method: Method::Invite,
            cseq: 2,
            from_tag: Tag::new("ja743ks76zlflH"),
            call_id: "1j9FpLxk3uxtm8tn@biloxi.example.com".to_string(),
        })
    );
    Ok(())
}
