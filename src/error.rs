use crate::transaction::key::TransactionKey;
use wasm_bindgen::prelude::*;
#[derive(Debug, Eq, PartialEq, Clone)]
pub enum Error {
    SipMessageError(String),
    TransportLayerError(String),
    TransactionError(String, TransactionKey),
    EndpointError(String),
    DialogError(String),
    Error(String),
}

impl Into<JsValue> for Error {
    fn into(self) -> JsValue {
        match self {
            Error::SipMessageError(e) => e.into(),
            Error::TransportLayerError(e) => e.into(),
            Error::TransactionError(e, key) => format!("{}: {}", e, key.to_string()).into(),
            Error::EndpointError(e) => e.into(),
            Error::DialogError(e) => e.into(),
            Error::Error(e) => e.into(),
        }
    }
}
impl From<rsip::Error> for Error {
    fn from(e: rsip::Error) -> Self {
        Error::SipMessageError(e.to_string())
    }
}

impl<T> From<tokio::sync::mpsc::error::SendError<T>> for Error {
    fn from(e: tokio::sync::mpsc::error::SendError<T>) -> Self {
        Error::Error(e.to_string())
    }
}
