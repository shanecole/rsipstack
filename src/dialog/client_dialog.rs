use super::dialog::DialogInnerRef;
use super::DialogId;
use crate::dialog::{authenticate::handle_client_authenticate, dialog::DialogState};
use crate::transaction::transaction::Transaction;
use crate::Result;
use rsip::prelude::HeadersExt;
use rsip::{SipMessage, StatusCode};
use tracing::{info, info_span, trace};

#[derive(Clone)]
pub struct ClientInviteDialog {
    pub(super) inner: DialogInnerRef,
}

impl ClientInviteDialog {
    pub fn new() -> Self {
        todo!()
    }

    pub fn id(&self) -> DialogId {
        self.inner.id.clone()
    }

    pub async fn cancel(&self) -> Result<()> {
        let request = self.inner.make_request(rsip::Method::Cancel, None, None)?;
        self.inner.do_request(&request).await?;
        Ok(())
    }

    pub async fn bye(&self) -> Result<()> {
        if !self.inner.is_confirmed() {
            return Ok(());
        }
        let request = self.inner.make_request(rsip::Method::Bye, None, None)?;
        let resp = self.inner.do_request(&request).await?;
        self.inner.transition(DialogState::Terminated(
            self.id(),
            resp.map(|r| r.status_code),
        ))?;
        Ok(())
    }

    pub async fn reinvite(&self) -> Result<()> {
        if !self.inner.is_confirmed() {
            return Ok(());
        }
        Ok(())
    }

    pub async fn info(&self) -> Result<()> {
        if !self.inner.is_confirmed() {
            return Ok(());
        }

        let request = self.inner.make_request(rsip::Method::Info, None, None)?;
        self.inner.do_request(&request).await?;
        self.inner
            .transition(DialogState::Info(self.id(), request))?;
        Ok(())
    }

    pub async fn process(&self, mut tx: Transaction) -> Result<()> {
        todo!()
    }

    pub async fn handle_invite(&self, mut tx: Transaction) -> Result<()> {
        let span = info_span!("client_dialog", dialog_id = %self.id());
        let _enter = span.enter();

        self.inner
            .tu_sender
            .lock()
            .unwrap()
            .replace(tx.tu_sender.clone());

        self.inner.transition(DialogState::Calling(self.id()))?;
        let mut auth_sent = false;

        while let Some(msg) = tx.receive().await {
            match msg {
                SipMessage::Request(_) => {}
                SipMessage::Response(resp) => match resp.status_code {
                    StatusCode::Trying => {
                        self.inner.transition(DialogState::Trying(self.id()))?;
                    }
                    StatusCode::Ringing | StatusCode::SessionProgress => {
                        self.inner.transition(DialogState::Early(self.id(), resp))?;
                    }
                    StatusCode::OK => {
                        let to_tag = resp.to_header()?.tag()?.ok_or(crate::Error::DialogError(
                            "received 200 response without to tag".to_string(),
                            self.id(),
                        ))?;
                        let ack = rsip::Request {
                            method: rsip::Method::Ack,
                            uri: tx.original.uri.clone(),
                            headers: tx.original.headers.clone(),
                            version: rsip::Version::V2,
                            body: Default::default(),
                        };
                        tx.send_ack(ack).await?;
                        self.inner
                            .transition(DialogState::Confirmed(self.id(), resp))?;
                    }
                    StatusCode::Decline => {
                        info!("received decline response: {}", resp.status_code);
                        self.inner.transition(DialogState::Terminated(
                            self.id(),
                            Some(resp.status_code),
                        ))?;
                    }
                    StatusCode::ProxyAuthenticationRequired | StatusCode::Unauthorized => {
                        if auth_sent {
                            info!("received {} response after auth sent", resp.status_code);
                            self.inner.transition(DialogState::Terminated(
                                self.id(),
                                Some(resp.status_code),
                            ))?;
                            break;
                        }
                        auth_sent = true;
                        if let Some(credential) = &self.inner.credential {
                            tx = handle_client_authenticate(
                                self.inner.increment_local_seq(),
                                tx,
                                resp,
                                credential,
                            )
                            .await?;
                            tx.send().await?;
                            continue;
                        } else {
                            info!("received 407 response without auth option");
                            self.inner.transition(DialogState::Terminated(
                                self.id(),
                                Some(resp.status_code),
                            ))?;
                        }
                    }
                    _ => match resp.status_code.kind() {
                        rsip::StatusCodeKind::Redirection => {
                            self.inner.transition(DialogState::Terminated(
                                self.id(),
                                Some(resp.status_code),
                            ))?;
                        }
                        rsip::StatusCodeKind::RequestFailure
                        | rsip::StatusCodeKind::ServerFailure
                        | rsip::StatusCodeKind::GlobalFailure => {
                            info!("received failure response: {}", resp.status_code);
                            self.inner.transition(DialogState::Terminated(
                                self.id(),
                                Some(resp.status_code),
                            ))?;
                        }
                        _ => {
                            info!("ignoring response: {}", resp.status_code);
                        }
                    },
                },
            }
        }
        trace!("process done");
        self.inner.tu_sender.lock().unwrap().take();
        Ok(())
    }
}
