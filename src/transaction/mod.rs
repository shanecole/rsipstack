use async_trait::async_trait;

pub mod server_invite;
pub mod client_invite;
pub mod server_non_invite;
pub mod client_non_invite;


#[async_trait]
pub trait ServerTransaction {
}

#[async_trait]
pub trait ClientTransaction: Send + Sync {
    async fn send(&self) -> Result<(), crate::error::Error>;
    async fn next(&self) -> Option<rsip::Response>;
}