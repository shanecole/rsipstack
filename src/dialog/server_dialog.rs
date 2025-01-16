use super::dialog::{Dialog, DialogInnerRef};
use super::DialogId;
use crate::dialog::dialog::DialogState;
use crate::transaction::transaction::{Transaction, TransactionEvent};
use crate::Result;
use rsip::prelude::HeadersExt;
use rsip::{Header, Request, SipMessage, StatusCode};
use std::sync::atomic::Ordering;
use tokio_util::sync::CancellationToken;
use tracing::{info, info_span, trace};

#[derive(Clone)]
pub struct ServerInviteDialog {
    pub(super) inner: DialogInnerRef,
}

impl ServerInviteDialog {
    pub fn id(&self) -> DialogId {
        self.inner.id.lock().unwrap().clone()
    }
    pub fn cancel_token(&self) -> &CancellationToken {
        &self.inner.cancel_token
    }
    pub fn initial_request(&self) -> &Request {
        &self.inner.initial_request
    }

    pub fn accept(&self, headers: Option<Vec<Header>>, body: Option<Vec<u8>>) -> Result<()> {
        if let Some(sender) = self.inner.tu_sender.lock().unwrap().as_ref() {
            let resp = self.inner.make_response(
                &self.inner.initial_request,
                rsip::StatusCode::OK,
                headers,
                body,
            );

            sender
                .send(TransactionEvent::Respond(resp))
                .map_err(Into::into)
        } else {
            Err(crate::Error::DialogError(
                "transaction is already terminated".to_string(),
                self.id(),
            ))
        }
    }

    pub fn reject(&self) -> Result<()> {
        if let Some(sender) = self.inner.tu_sender.lock().unwrap().as_ref() {
            let resp = self.inner.make_response(
                &self.inner.initial_request,
                rsip::StatusCode::Decline,
                None,
                None,
            );
            sender
                .send(TransactionEvent::Respond(resp))
                .map_err(Into::into)
        } else {
            Err(crate::Error::DialogError(
                "transaction is already terminated".to_string(),
                self.id(),
            ))
        }
    }

    pub async fn bye(&self) -> Result<()> {
        if !self.inner.is_confirmed() {
            return Ok(());
        }
        let request = self
            .inner
            .make_request(rsip::Method::Bye, None, None, None, None)?;
        let resp = self.inner.do_request(&request).await?;
        self.inner.transition(DialogState::Terminated(
            self.id(),
            resp.map(|r| r.status_code),
        ))?;
        self.inner
            .transition(DialogState::Terminated(self.id(), None))?;
        Ok(())
    }

    pub async fn reinvite(&self) -> Result<()> {
        todo!()
    }

    pub async fn info(&self) -> Result<()> {
        if !self.inner.is_confirmed() {
            return Ok(());
        }
        let request = self
            .inner
            .make_request(rsip::Method::Info, None, None, None, None)?;
        self.inner.do_request(&request).await?;
        Ok(())
    }

    pub async fn handle(&mut self, mut tx: Transaction) -> Result<()> {
        let span = info_span!("server_invite_dialog", dialog_id = %self.id());
        let _enter = span.enter();

        trace!(
            "handle request: {:?} state:{}",
            tx.original,
            self.inner.state.lock().unwrap()
        );

        let cseq = tx.original.cseq_header()?.seq()?;
        if cseq < self.inner.remote_seq.load(Ordering::Relaxed) {
            info!(
                "received old request remote_seq: {} > {}",
                self.inner.remote_seq.load(Ordering::Relaxed),
                cseq
            );
            tx.reply(rsip::StatusCode::ServerInternalError).await?;
            return Ok(());
        }

        self.inner.remote_seq.store(cseq, Ordering::Relaxed);

        if self.inner.is_confirmed() {
            match tx.original.method {
                rsip::Method::Invite => {}
                rsip::Method::Bye => return self.handle_bye(tx).await,
                rsip::Method::Info => return self.handle_info(tx).await,
                _ => {
                    info!("invalid request method: {:?}", tx.original.method);
                    tx.reply(rsip::StatusCode::MethodNotAllowed).await?;
                    return Err(crate::Error::DialogError(
                        "invalid request".to_string(),
                        self.id(),
                    ));
                }
            }
        } else {
            match tx.original.method {
                rsip::Method::Ack => {
                    if let Some(sender) = self.inner.tu_sender.lock().unwrap().as_ref() {
                        sender
                            .send(TransactionEvent::Received(
                                tx.original.clone().into(),
                                tx.connection.clone(),
                            ))
                            .ok();
                    }
                    return Ok(());
                }
                _ => {}
            }
        }
        self.handle_invite(tx).await
    }

    async fn handle_bye(&mut self, mut tx: Transaction) -> Result<()> {
        info!("received bye");
        self.inner
            .transition(DialogState::Terminated(self.id(), None))?;
        tx.reply(rsip::StatusCode::OK).await?;
        Ok(())
    }

    async fn handle_info(&mut self, mut tx: Transaction) -> Result<()> {
        self.inner
            .transition(DialogState::Info(self.id(), tx.original.clone()))?;
        tx.reply(rsip::StatusCode::OK).await?;
        Ok(())
    }

    async fn handle_invite(&mut self, mut tx: Transaction) -> Result<()> {
        self.inner
            .tu_sender
            .lock()
            .unwrap()
            .replace(tx.tu_sender.clone());

        if !self.inner.is_confirmed() {
            self.inner.transition(DialogState::Calling(self.id()))?;
        }

        tx.send_trying().await?;

        while let Some(msg) = tx.receive().await {
            match msg {
                SipMessage::Request(req) => match req.method {
                    rsip::Method::Ack => {
                        info!("received ack");
                        let last_response = tx.last_response.clone().unwrap_or_default();
                        self.inner
                            .transition(DialogState::Confirmed(self.id(), last_response.into()))?;
                    }
                    rsip::Method::Cancel => {
                        info!("received cancel");
                        tx.reply(rsip::StatusCode::RequestTerminated).await?;
                        self.inner.transition(DialogState::Terminated(
                            self.id(),
                            Some(StatusCode::RequestTerminated),
                        ))?;
                    }
                    _ => {}
                },
                SipMessage::Response(_) => {}
            }
        }
        trace!("process done");
        self.inner.tu_sender.lock().unwrap().take();
        Ok(())
    }
}

impl TryFrom<&Dialog> for ServerInviteDialog {
    type Error = crate::Error;

    fn try_from(dlg: &Dialog) -> Result<Self> {
        match dlg {
            Dialog::ServerInvite(dlg) => Ok(dlg.clone()),
            _ => Err(crate::Error::DialogError(
                "Dialog is not a ServerInviteDialog".to_string(),
                dlg.id(),
            )),
        }
    }
}
