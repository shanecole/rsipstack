
use rsip::prelude::HeadersExt;
use crate::Result;

pub mod server_invite;
pub mod client_invite;
pub mod server;
pub mod client;

pub(crate) type ClientTransactionSender = tokio::sync::mpsc::UnboundedSender<rsip::Response>;
pub(crate) type ClientTransactionReceiver = tokio::sync::mpsc::UnboundedReceiver<rsip::Response>;
pub(crate) type OutgoingSender = tokio::sync::mpsc::UnboundedSender<(String, rsip::SipMessage)>;
pub(crate) type OutgoingReceiver = tokio::sync::mpsc::UnboundedReceiver<(String, rsip::SipMessage)>;

pub struct ClientTransaction {
    pub key: String,
    pub origin: rsip::Request,
    pub receiver: ClientTransactionReceiver,
    pub outgoing_tx: OutgoingSender,
}

pub struct ServerTransaction {
    pub origin: rsip::Request,
}

pub fn gen_transaction_key(req: &rsip::Request) -> Result<String> {
    let seq = req.cseq_header()?.seq()?;
    let form = req.from_header()?;
    let to = req.to_header()?;
    let call_id = req.call_id_header()?;
    let combined = format!("{}-{}-{}", form, to, call_id);
    Ok(format!("{}-{}-{}", req.method().to_string(), seq, combined))
}