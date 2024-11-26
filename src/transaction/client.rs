use super::ClientTransaction;
use crate::Result;

impl Drop for ClientTransaction {
    fn drop(&mut self) {
        
    }
}

impl ClientTransaction {
    pub async fn send(&self) -> Result<()> {
        self.outgoing_tx.send((self.key.to_string(), self.origin.clone().into()))?;
        Ok(())
    }

    pub async fn receive(&self) -> Result<rsip::Response> {
        todo!("Implement this method")
    }
}