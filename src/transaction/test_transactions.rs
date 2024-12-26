use crate::{transaction::key::TransactionKey, Result};
use rsip::headers::*;
#[test]
fn test_transaction_key() -> Result<()> {
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
