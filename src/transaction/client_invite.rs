use super::transaction::{TransactionInnerRef, TransactionTimer, T1};
use crate::Result;
use rsip::Request;
#[derive(Clone)]
pub(crate) struct ClientInviteHandler {
    pub inner: TransactionInnerRef,
    pub last_ack: Option<Request>,
}

impl ClientInviteHandler {
    pub(super) async fn on_timer(&self, timer: &TransactionTimer) -> Result<()> {
        match timer {
            TransactionTimer::TimerA(key) => {
                //let value = 2 * T1;
                //self.inner.core.timers.timeout_at(value, key);
                // If Timer A fires while in the "Calling" state, the client transaction SHOULD retransmit the request by passing it to the transport layer, and MUST reset the Timer A with a value of 2*T1.
                // If Timer A fires while in the "Proceeding" state, the client transaction SHOULD retransmit the request by passing it to the transport layer, and MUST reset the Timer A with a value of 2*T1.
                // If Timer A fires while in the "Completed" state, the client transaction MUST transition to the "Terminated" state.
                // If Timer A fires while in the "Terminated" state, the client transaction MUST be destroyed.
            }
            _ => {}
        }

        Ok(())
    }
}
