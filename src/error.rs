use crate::{dialog::DialogId, transaction::key::TransactionKey, transport::SipAddr};
use std::env::VarError;
use thiserror::Error as ThisError;
use wasm_bindgen::prelude::*;

#[derive(Debug, ThisError)]
pub enum Error {
    #[error("SIP message error: {0}")]
    SipMessageError(#[from] rsip::Error),

    #[error("DNS resolution error: {0}")]
    DnsResolutionError(String),

    #[error("Transport layer error: {0}: {1}")]
    TransportLayerError(String, SipAddr),

    #[error("Transaction error: {0}: {1}")]
    TransactionError(String, TransactionKey),

    #[error("Endpoint error: {0}")]
    EndpointError(String),

    #[error("Dialog error: {0}: {1}")]
    DialogError(String, DialogId),

    #[error("I/O error: {0}")]
    IoError(#[from] std::io::Error),

    #[error("Format error: {0}")]
    FormatError(#[from] std::fmt::Error),

    #[error("Environment variable error: {0}")]
    VarError(#[from] VarError),

    #[error("Address parse error: {0}")]
    AddrParseError(#[from] std::net::AddrParseError),

    #[error("TLS error: {0}")]
    TlsError(#[from] tokio_rustls::rustls::Error),

    #[error("WebSocket error: {0}")]
    WebSocketError(#[from] tokio_tungstenite::tungstenite::Error),

    #[error("Channel send error: {0}")]
    ChannelSendError(String),

    #[error("Broadcast receive error: {0}")]
    BroadcastRecvError(#[from] tokio::sync::broadcast::error::RecvError),

    #[error("Error: {0}")]
    Error(String),
}

impl From<Error> for JsValue {
    fn from(e: Error) -> Self {
        e.to_string().into()
    }
}

impl<T> From<tokio::sync::mpsc::error::SendError<T>> for Error {
    fn from(e: tokio::sync::mpsc::error::SendError<T>) -> Self {
        Error::ChannelSendError(e.to_string())
    }
}
