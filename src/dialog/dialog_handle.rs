use crate::{
    transaction::{
        transaction::{TransactionEvent, TransactionEventSender},
        TransactionType,
    },
    Result,
};
use rsip::Header;

use super::dialog::{Dialog, DialogStateSender};

#[derive(Clone)]
pub enum DialogHandle {
    ClientInvite(ClientInviteDialogHandle),
    ServerInvite(ServerInviteDialogHandle),
}

#[derive(Clone)]
pub struct ClientInviteDialogHandle {
    pub(super) state_sender: DialogStateSender,
    pub(super) tu_sender: TransactionEventSender,
}

impl ClientInviteDialogHandle {
    pub fn cancel(&self) -> Result<()> {
        todo!()
    }
    pub fn bye(&self) -> Result<()> {
        todo!()
    }
    pub async fn reinvite(&self) -> Result<()> {
        todo!()
    }
    pub async fn info(&self) -> Result<()> {
        todo!()
    }
}

#[derive(Clone)]
pub struct ServerInviteDialogHandle {
    pub(super) state_sender: DialogStateSender,
    pub(super) tu_sender: TransactionEventSender,
}

impl ServerInviteDialogHandle {
    pub fn accept(&self, body: Vec<u8>, headers: Option<Vec<Header>>) -> Result<()> {
        self.tu_sender
            .send(TransactionEvent::Respond(
                rsip::StatusCode::OK,
                headers,
                Some(body),
            ))
            .map_err(Into::into)
    }

    pub fn reject(&self) -> Result<()> {
        self.tu_sender
            .send(TransactionEvent::Respond(
                rsip::StatusCode::BusyHere,
                None,
                None,
            ))
            .map_err(Into::into)
    }

    pub fn bye(&self) -> Result<()> {
        // create new bye transaction
        todo!()
    }

    pub async fn reinvite(&self) -> Result<()> {
        todo!()
    }
    pub async fn info(&self) -> Result<()> {
        todo!()
    }
}

impl TryFrom<&Dialog> for DialogHandle {
    type Error = crate::Error;

    fn try_from(dialog: &Dialog) -> Result<Self> {
        match dialog.initiator.transaction_type {
            TransactionType::ClientInvite => {
                Ok(DialogHandle::ClientInvite(ClientInviteDialogHandle {
                    state_sender: dialog.state_sender.clone(),
                    tu_sender: dialog.initiator.tu_sender.clone(),
                }))
            }
            TransactionType::ServerInvite => {
                Ok(DialogHandle::ServerInvite(ServerInviteDialogHandle {
                    state_sender: dialog.state_sender.clone(),
                    tu_sender: dialog.initiator.tu_sender.clone(),
                }))
            }
            _ => Err(crate::Error::DialogError(
                "DialogHandle not available for this transaction type".to_string(),
            )),
        }
    }
}
