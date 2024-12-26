use super::transaction::{TransactionInnerRef, TransactionTimer};
use crate::Result;
use rsip::Request;
#[derive(Clone)]
pub(crate) struct ServerInviteHandler {
    pub inner: TransactionInnerRef,
    pub last_ack: Option<Request>,
}
impl ServerInviteHandler {
    pub(super) async fn on_timer(&self, timer: &TransactionTimer) -> Result<()> {
        Ok(())
    }
}
