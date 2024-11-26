// A SIP stack in Rust
pub type Result<T> = std::result::Result<T, crate::error::Error>;
pub use crate::error::Error;
pub mod endpoint;
pub mod transaction;
pub mod transport;
pub mod dialog;
pub mod error;
pub mod useragent;
pub use endpoint::EndpointBuilder;
