use super::transaction::{TransactionInnerRef, TransactionTimer};
use crate::Result;
use rsip::Request;
#[derive(Clone)]
pub(crate) struct ServerNonInviteHandler {
    pub inner: TransactionInnerRef,
}
impl ServerNonInviteHandler {
    pub(super) async fn on_timer(&self, timer: &TransactionTimer) -> Result<()> {
        Ok(())
    }
}
