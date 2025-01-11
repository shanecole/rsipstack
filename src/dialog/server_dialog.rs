use super::dialog::DialogInnerRef;
use crate::dialog::dialog::DialogState;
use crate::transaction::transaction::{Transaction, TransactionEvent};
use crate::Result;
use rsip::{Header, SipMessage, StatusCode};
use tracing::{info, info_span, trace};

#[derive(Clone)]
pub struct ServerInviteDialog {
    pub(super) inner: DialogInnerRef,
}

impl ServerInviteDialog {
    pub fn accept(&self, headers: Option<Vec<Header>>, body: Vec<u8>) -> Result<()> {
        if let Some(sender) = self.inner.tu_sender.lock().unwrap().as_ref() {
            let resp = self.inner.make_response(
                &self.inner.initial_request,
                rsip::StatusCode::OK,
                headers,
                Some(body),
            );

            sender
                .send(TransactionEvent::Respond(resp))
                .map_err(Into::into)
        } else {
            Err(crate::Error::DialogError(
                "transaction is already terminated".to_string(),
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
            ))
        }
    }

    pub async fn bye(&self) -> Result<()> {
        if !self.inner.is_confirmed() {
            return Ok(());
        }
        let request = self.inner.make_request(rsip::Method::Bye, None, None);
        let resp = self.inner.do_request(&request).await?;
        self.inner
            .transition(DialogState::Terminated(resp.map(|r| r.status_code)))?;
        self.inner.transition(DialogState::Terminated(None))?;
        Ok(())
    }

    pub async fn reinvite(&self) -> Result<()> {
        todo!()
    }

    pub async fn info(&self) -> Result<()> {
        if !self.inner.is_confirmed() {
            return Ok(());
        }
        let request = self.inner.make_request(rsip::Method::Info, None, None);
        self.inner.do_request(&request).await?;
        Ok(())
    }

    pub async fn handle_invite(&mut self, mut tx: Transaction) -> Result<()> {
        let span = info_span!("server_invite_dialog", dialog_id = %self.inner.id.lock().unwrap());
        let _enter = span.enter();

        self.inner
            .tu_sender
            .lock()
            .unwrap()
            .replace(tx.tu_sender.clone());

        self.inner
            .transition(DialogState::Calling(tx.original.clone()))?;

        while let Some(msg) = tx.receive().await {
            match msg {
                SipMessage::Request(req) => match req.method {
                    rsip::Method::Ack => {
                        info!("received ack");
                        let last_response = tx.last_response.clone().unwrap_or_default();
                        self.inner
                            .transition(DialogState::Confirmed(last_response.into()))?;
                    }
                    rsip::Method::Bye => {
                        info!("received bye");
                        self.inner.transition(DialogState::Terminated(None))?;
                    }
                    rsip::Method::Cancel => {
                        info!("received cancel");
                        self.inner.transition(DialogState::Terminated(Some(
                            StatusCode::RequestTerminated,
                        )))?;
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
