use super::ClientTransaction;
use async_trait::async_trait;
pub struct ClientNonInviteTransaction {
    req: rsip::Request,
}

impl ClientNonInviteTransaction{
    pub fn new(req: rsip::Request) -> Self {
        ClientNonInviteTransaction {
            req
        }
    }
}

#[async_trait]
impl ClientTransaction  for   ClientNonInviteTransaction {
     async fn send(&self) -> Result<(),crate::error::Error> {
        Err(crate::error::Error::Error("Not implemented".to_string()))
    }

     async fn next(&self) -> Option<rsip::Response> {
        None
    }
}