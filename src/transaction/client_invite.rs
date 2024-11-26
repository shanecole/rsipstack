use crate::Result;
use super::ClientTransaction;

impl ClientTransaction {
    pub async fn ack() -> Result<()> {
        Ok(())
    }
}