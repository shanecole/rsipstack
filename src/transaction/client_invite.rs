use super::ClientTransaction;
use async_trait::async_trait;


pub struct ClientInviteTransaction {
    req: rsip::Request,
}

impl ClientInviteTransaction{
    pub fn new(req: rsip::Request) -> Self {
        ClientInviteTransaction {
            req
        }
    }
}

#[async_trait]
impl ClientTransaction for ClientInviteTransaction {
    async fn send(&self) -> Result<(), crate::error::Error> {
        Err(crate::error::Error::Error("Not implemented".to_string()))
    }

    async fn next(&self) -> Option<rsip::Response> {
        None
    }
}