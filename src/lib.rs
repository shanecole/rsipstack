// A SIP stack in Rust
pub type Result<T> = std::result::Result<T, crate::error::Error>;
pub use crate::error::Error;
pub mod dialog;
pub mod error;
pub mod transaction;
pub mod transport;
pub use transaction::EndpointBuilder;

const USER_AGENT: &str = "rsipstack/0.1";
