use std::sync::Arc;

use rsip::{
    prelude::{HasHeaders, HeadersExt, ToTypedHeader},
    Header, Response, SipMessage, StatusCode,
};
use tracing::info;

use super::authenticate::{handle_client_authenticate, Credential};
use crate::{
    transaction::{endpoint::Endpoint, random_text, TO_TAG_LEN},
    Result,
};

pub struct Registration {
    pub last_seq: u32,
    pub useragent: Arc<Endpoint>,
    pub credential: Option<Credential>,
    pub contact: Option<rsip::typed::Contact>,
}

impl Registration {
    pub fn new(useragent: Arc<Endpoint>, credential: Option<Credential>) -> Self {
        Self {
            last_seq: 0,
            useragent,
            credential,
            contact: None,
        }
    }

    pub fn expires(&self) -> u32 {
        self.contact
            .as_ref()
            .and_then(|c| c.expires())
            .map(|e| e.seconds().unwrap_or(50))
            .unwrap_or(50)
    }

    pub async fn register(&mut self, server: &String) -> Result<Response> {
        self.last_seq += 1;

        let recipient = rsip::Uri::try_from(format!("sip:{}", server))?;

        let mut to = rsip::typed::To {
            display_name: None,
            uri: recipient.clone(),
            params: vec![],
        };

        if let Some(cred) = &self.credential {
            to.uri.auth = Some(rsip::auth::Auth {
                user: cred.username.clone(),
                password: None,
            });
        }

        let form = rsip::typed::From {
            display_name: None,
            uri: to.uri.clone(),
            params: vec![],
        }
        .with_tag(random_text(TO_TAG_LEN).into());

        let contact = self
            .contact
            .clone()
            .unwrap_or_else(|| rsip::typed::Contact {
                display_name: None,
                uri: to.uri.clone(),
                params: vec![],
            });

        let mut request =
            self.useragent
                .make_request(rsip::Method::Register, recipient, form, to, self.last_seq);

        request.headers.unique_push(contact.into());

        let mut tx = self.useragent.client_transaction(request)?;
        tx.send().await?;
        let mut auth_sent = false;

        while let Some(msg) = tx.receive().await {
            match msg {
                SipMessage::Response(resp) => match resp.status_code {
                    StatusCode::Trying => {
                        continue;
                    }
                    StatusCode::ProxyAuthenticationRequired | StatusCode::Unauthorized => {
                        if auth_sent {
                            info!("received {} response after auth sent", resp.status_code);
                            return Ok(resp);
                        }

                        if let Some(cred) = &self.credential {
                            self.last_seq += 1;
                            tx = handle_client_authenticate(self.last_seq, tx, resp, cred).await?;
                            tx.send().await?;
                            auth_sent = true;
                            continue;
                        } else {
                            info!("received {} response without credential", resp.status_code);
                            return Ok(resp);
                        }
                    }
                    _ => {
                        info!("registration do_request done: {:?}", resp.status_code);
                        if let Ok(c) = resp.contact_header() {
                            c.clone().into_typed().map(|c| self.contact = Some(c)).ok();
                        }
                        return Ok(resp);
                    }
                },
                _ => break,
            }
        }
        return Err(crate::Error::DialogError(
            "registration transaction is already terminated".to_string(),
        ));
    }
}
