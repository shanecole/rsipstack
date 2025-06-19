use crate::{dialog::DialogId, transaction::key::TransactionKey, transport::SipAddr};
use std::env::VarError;
use wasm_bindgen::prelude::*;

#[derive(Debug, Eq, PartialEq, Clone)]
pub enum Error {
    SipMessageError(String),
    DnsResolutionError(String),
    TransportLayerError(String, SipAddr),
    TransactionError(String, TransactionKey),
    EndpointError(String),
    DialogError(String, DialogId),
    Error(String),
}

impl std::fmt::Display for Error {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Error::SipMessageError(e) => write!(f, "SIP message error: {}", e),
            Error::DnsResolutionError(e) => write!(f, "DNS resolution error: {}", e),
            Error::TransportLayerError(e, addr) => {
                write!(f, "Transport layer error: {}: {}", e, addr)
            }
            Error::TransactionError(e, key) => write!(f, "Transaction error: {}: {}", e, key),
            Error::EndpointError(e) => write!(f, "Endpoint error: {}", e),
            Error::DialogError(e, id) => write!(f, "Dialog error: {}: {}", e, id),
            Error::Error(e) => write!(f, "Error: {}", e),
        }
    }
}

impl Into<JsValue> for Error {
    fn into(self) -> JsValue {
        match self {
            Error::DnsResolutionError(e) => e.into(),
            Error::SipMessageError(e) => e.into(),
            Error::TransportLayerError(e, _) => e.into(),
            Error::TransactionError(e, key) => format!("{}: {}", e, key.to_string()).into(),
            Error::EndpointError(e) => e.into(),
            Error::DialogError(e, id) => format!("{}: {}", e, id.to_string()).into(),
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

impl From<tokio::sync::broadcast::error::RecvError> for Error {
    fn from(e: tokio::sync::broadcast::error::RecvError) -> Self {
        Error::Error(e.to_string())
    }
}
impl From<std::io::Error> for Error {
    fn from(e: std::io::Error) -> Self {
        Error::Error(e.to_string())
    }
}

impl From<std::fmt::Error> for Error {
    fn from(e: std::fmt::Error) -> Self {
        Error::Error(e.to_string())
    }
}
impl From<VarError> for Error {
    fn from(e: VarError) -> Self {
        Error::Error(e.to_string())
    }
}

impl From<std::net::AddrParseError> for Error {
    fn from(e: std::net::AddrParseError) -> Self {
        Error::Error(e.to_string())
    }
}

impl From<tokio_rustls::rustls::Error> for Error {
    fn from(e: tokio_rustls::rustls::Error) -> Self {
        Error::Error(e.to_string())
    }
}

impl From<tokio_tungstenite::tungstenite::Error> for Error {
    fn from(e: tokio_tungstenite::tungstenite::Error) -> Self {
        Error::Error(e.to_string())
    }
}
