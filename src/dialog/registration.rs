use super::{
    authenticate::{handle_client_authenticate, Credential},
    DialogId,
};
use crate::{
    transaction::{
        endpoint::EndpointInnerRef,
        key::{TransactionKey, TransactionRole},
        make_tag,
        transaction::Transaction,
    },
    transport::SipAddr,
    Error, Result,
};
use get_if_addrs::get_if_addrs;
use rsip::{HostWithPort, Response, SipMessage, StatusCode};
use rsip_dns::trust_dns_resolver::TokioAsyncResolver;
use rsip_dns::ResolvableExt;
use std::net::IpAddr;
use tracing::info;

pub struct Registration {
    pub last_seq: u32,
    pub endpoint: EndpointInnerRef,
    pub credential: Option<Credential>,
    pub contact: Option<rsip::typed::Contact>,
    pub allow: rsip::headers::Allow,
}

impl Registration {
    pub fn new(endpoint: EndpointInnerRef, credential: Option<Credential>) -> Self {
        Self {
            last_seq: 0,
            endpoint,
            credential,
            contact: None,
            allow: Default::default(),
        }
    }

    pub fn expires(&self) -> u32 {
        self.contact
            .as_ref()
            .and_then(|c| c.expires())
            .map(|e| e.seconds().unwrap_or(50))
            .unwrap_or(50)
    }

    fn get_first_non_loopback_interface() -> Result<IpAddr> {
        get_if_addrs()?
            .iter()
            .find(|i| !i.is_loopback())
            .map(|i| match i.addr {
                get_if_addrs::IfAddr::V4(ref addr) => Ok(std::net::IpAddr::V4(addr.ip)),
                _ => Err(Error::Error("No IPv4 address found".to_string())),
            })
            .unwrap_or(Err(Error::Error("No interface found".to_string())))
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
        .with_tag(make_tag());

        let first_addr = {
            let mut addr =
                SipAddr::from(HostWithPort::from(Self::get_first_non_loopback_interface()?));
            let context = rsip_dns::Context::initialize_from(
                recipient.clone(),
                rsip_dns::AsyncTrustDnsClient::new(
                    TokioAsyncResolver::tokio(Default::default(), Default::default()).unwrap(),
                ),
                rsip_dns::SupportedTransports::any(),
            )?;

            let mut lookup = rsip_dns::Lookup::from(context);
            match lookup.resolve_next().await {
                Some(target) => {
                    addr.r#type = Some(target.transport);
                    addr
                }
                None => {
                    Err(crate::Error::DnsResolutionError(format!(
                        "DNS resolution error: {}",
                        recipient
                    )))
                }?,
            }
        };
        let contact = self
            .contact
            .clone()
            .unwrap_or_else(|| rsip::typed::Contact {
                display_name: None,
                uri: rsip::Uri {
                    auth: to.uri.auth.clone(),
                    scheme: Some(rsip::Scheme::Sip),
                    host_with_port: first_addr.clone().into(),
                    params: vec![],
                    headers: vec![],
                },
                params: vec![],
            });
        let via = self.endpoint.get_via(Some(first_addr.clone()), None)?;
        let mut request = self.endpoint.make_request(
            rsip::Method::Register,
            recipient,
            via,
            form,
            to,
            self.last_seq,
        );

        request.headers.unique_push(contact.into());
        request.headers.unique_push(self.allow.clone().into());

        let key = TransactionKey::from_request(&request, TransactionRole::Client)?;
        let mut tx = Transaction::new_client(key, request, self.endpoint.clone(), None);

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
                        return Ok(resp);
                    }
                },
                _ => break,
            }
        }
        return Err(crate::Error::DialogError(
            "registration transaction is already terminated".to_string(),
            DialogId::try_from(&tx.original)?,
        ));
    }
}
