use super::{authenticate::Credential, dialog::DialogInnerRef};
use crate::dialog::{authenticate::handle_client_proxy_authenticate, dialog::DialogState};
use crate::transaction::transaction::Transaction;
use crate::Result;
use rsip::prelude::HeadersExt;
use rsip::{SipMessage, StatusCode};
use tracing::{info, info_span, trace};

#[derive(Clone)]
pub struct ClientInviteDialog {
    pub auth_option: Option<Credential>,
    pub(super) inner: DialogInnerRef,
}

impl ClientInviteDialog {
    pub fn new() -> Self {
        todo!()
    }

    pub async fn cancel(&self) -> Result<()> {
        let request = self.inner.make_request(rsip::Method::Cancel, None, None);
        self.inner.do_request(&request).await?;
        Ok(())
    }

    pub async fn bye(&self) -> Result<()> {
        if !self.inner.is_confirmed() {
            return Ok(());
        }
        let request = self.inner.make_request(rsip::Method::Bye, None, None);
        let resp = self.inner.do_request(&request).await?;
        self.inner
            .transition(DialogState::Terminated(resp.map(|r| r.status_code)))?;
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

        let request = self.inner.make_request(rsip::Method::Info, None, None);
        self.inner.do_request(&request).await?;
        self.inner.transition(DialogState::Info(request))?;
        Ok(())
    }

    pub async fn process(&self, mut tx: Transaction) -> Result<()> {
        todo!()
    }

    pub async fn handle_invite(&self, mut tx: Transaction) -> Result<()> {
        let span = info_span!("client_dialog", dialog_id = %self.inner.id.lock().unwrap());
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
                SipMessage::Request(_) => {}
                SipMessage::Response(resp) => match resp.status_code {
                    StatusCode::Trying => {
                        self.inner.transition(DialogState::Trying)?;
                    }
                    StatusCode::Ringing | StatusCode::SessionProgress => {
                        if let Some(to_tag) = resp.to_header()?.tag()? {
                            self.inner.update_remote_tag(to_tag); // ealy dialog
                        }
                        self.inner.transition(DialogState::Early(resp))?;
                    }
                    StatusCode::OK => {
                        let to_tag = resp.to_header()?.tag()?.ok_or(crate::Error::DialogError(
                            "received 200 response without to tag".to_string(),
                        ))?;
                        let ack = rsip::Request {
                            method: rsip::Method::Ack,
                            uri: tx.original.uri.clone(),
                            headers: tx.original.headers.clone(),
                            version: rsip::Version::V2,
                            body: Default::default(),
                        };
                        tx.send_ack(ack).await?;
                        self.inner.update_remote_tag(to_tag);
                        self.inner.transition(DialogState::Confirmed(resp))?;
                    }
                    StatusCode::Decline => {
                        info!("received decline response: {}", resp.status_code);
                        self.inner
                            .transition(DialogState::Terminated(Some(resp.status_code)))?;
                    }
                    StatusCode::ProxyAuthenticationRequired => {
                        if let Some(auth_option) = &self.auth_option {
                            tx = handle_client_proxy_authenticate(
                                self.inner.increment_local_seq(),
                                tx,
                                resp,
                                auth_option,
                            )
                            .await?;
                            tx.send().await?;
                            continue;
                        } else {
                            info!("received 407 response without auth option");
                            self.inner
                                .transition(DialogState::Terminated(Some(resp.status_code)))?;
                        }
                    }
                    _ => match resp.status_code.kind() {
                        rsip::StatusCodeKind::Redirection => {
                            self.inner
                                .transition(DialogState::Terminated(Some(resp.status_code)))?;
                        }
                        rsip::StatusCodeKind::RequestFailure
                        | rsip::StatusCodeKind::ServerFailure
                        | rsip::StatusCodeKind::GlobalFailure => {
                            info!("received failure response: {}", resp.status_code);
                            self.inner
                                .transition(DialogState::Terminated(Some(resp.status_code)))?;
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
