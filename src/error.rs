#[derive(Debug, Eq, PartialEq, Clone)]
pub enum Error {
    SipMessageError(String),
    TransportLayerError(String),
    TransactionError(String),
    EndpointError(String),
    DialogError(String),
    Error(String),
}
