use super::{
    dialog_handle::{ClientInviteDialogHandle, DialogHandle, ServerInviteDialogHandle},
    DialogID,
};
use crate::{
    transaction::{transaction::Transaction, TransactionType},
    Result,
};
use rsip::{typed::Route, SipMessage};
use std::sync::atomic::AtomicU32;
use tokio::{
    select,
    sync::mpsc::{UnboundedReceiver, UnboundedSender},
};
use tokio_util::sync::CancellationToken;

/// DialogState is the state of the dialog
pub enum DialogState {
    Calling(rsip::Request),
    Trying,
    Early(rsip::Response),
    Confirmed(rsip::Response),
    Updated(rsip::Response),
    Notify(rsip::Request),
    Info(rsip::Request),
    Terminated,
}

pub type DialogStateReceiver = UnboundedReceiver<DialogState>;
pub type DialogStateSender = UnboundedSender<DialogState>;

pub struct Dialog {
    pub id: DialogID,
    pub state: DialogState,
    pub local_seq: AtomicU32,
    pub local_tag: String,
    pub local_uri: rsip::Uri,
    pub remote_uri: Option<rsip::Uri>,
    pub remote_tag: Option<String>,
    pub remote_seq: AtomicU32,
    pub route_sets: Vec<Route>,
    pub secure: bool,

    pub(super) initiator: Transaction,
    pub(super) state_sender: DialogStateSender,
}

impl Dialog {
    pub async fn process(&mut self) -> Result<()> {
        while let Some(msg) = self.initiator.receive().await {
            match msg {
                SipMessage::Request(req) => match req.method {
                    rsip::Method::Ack => {
                        let last_response = self.initiator.last_response.clone();
                        self.state_sender
                            .send(DialogState::Confirmed(last_response.unwrap_or_default()))?;
                    }
                    rsip::Method::Cancel => {
                        self.state_sender.send(DialogState::Terminated)?;
                    }
                    _ => {}
                },
                SipMessage::Response(resp) => match resp.status_code {
                    rsip::StatusCode::Trying => {
                        self.state_sender.send(DialogState::Trying)?;
                    }
                    rsip::StatusCode::Ringing => {
                        self.state_sender.send(DialogState::Early(resp))?;
                    }
                    rsip::StatusCode::OK => {
                        if self.initiator.transaction_type == TransactionType::ClientInvite {
                            let ack = rsip::Request {
                                method: rsip::Method::Ack,
                                uri: self.initiator.original.uri.clone(),
                                headers: self.initiator.original.headers.clone(),
                                version: rsip::Version::V2,
                                body: Default::default(),
                            };
                            self.initiator.send_ack(ack).await?;
                        }
                        self.state_sender.send(DialogState::Confirmed(resp))?;
                    }
                    _ => {}
                },
            }
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    pub async fn test_dialog() {
        //todo!()
    }
}
