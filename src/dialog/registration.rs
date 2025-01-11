use std::sync::Arc;

use rsip::{Response, SipMessage, StatusCode};
use tracing::info;

use super::authenticate::{handle_client_authenticate, Credential};
use crate::{
    transaction::{endpoint::Endpoint, random_text, TO_TAG_LEN},
    Result,
};

pub struct Registration {
    pub last_seq: u32,
    pub useragent: Arc<Endpoint>,
    pub auth_option: Option<Credential>,
}

impl Registration {
    pub fn new(useragent: Arc<Endpoint>, auth_option: Option<Credential>) -> Self {
        Self {
            last_seq: 0,
            useragent,
            auth_option,
        }
    }

    pub async fn register(&mut self, server: &String) -> Result<Response> {
        self.last_seq += 1;

        let recipient = rsip::Uri::try_from(format!("sip:{}", server))?;

        let mut to = rsip::typed::To {
            display_name: None,
            uri: recipient.clone(),
            params: vec![],
        };

        if let Some(cred) = &self.auth_option {
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

        let request =
            self.useragent
                .make_request(rsip::Method::Register, recipient, form, to, self.last_seq);

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
                            return Ok(Response::default());
                        }

                        if let Some(cred) = &self.auth_option {
                            self.last_seq += 1;
                            tx = handle_client_authenticate(self.last_seq, tx, resp, cred).await?;
                            tx.send().await?;
                            auth_sent = true;
                            continue;
                        } else {
                            info!("received {} response without auth option", resp.status_code);
                            return Ok(Response::default());
                        }
                    }
                    _ => {
                        info!("dialog do_request done: {:?}", resp.status_code);
                        return Ok(resp);
                    }
                },
                _ => break,
            }
        }
        return Err(crate::Error::DialogError(
            "transaction is already terminated".to_string(),
        ));
    }
}
