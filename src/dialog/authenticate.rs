use crate::transaction::transaction::Transaction;
use crate::transaction::{make_via_branch, random_text, CNONCE_LEN};
use crate::Result;
use rsip::headers::auth::AuthQop;
use rsip::prelude::{HasHeaders, HeadersExt, ToTypedHeader};
use rsip::services::DigestGenerator;
use rsip::typed::{Authorization, ProxyAuthorization};
use rsip::{Header, Param, Response};

#[derive(Clone)]
pub struct Credential {
    pub username: String,
    pub password: String,
}

pub async fn handle_client_authenticate(
    new_seq: u32,
    tx: Transaction,
    resp: Response,
    cred: &Credential,
) -> Result<Transaction> {
    let header = match resp.www_authenticate_header() {
        Some(h) => Header::WwwAuthenticate(h.clone()),
        None => {
            let proxy_header = rsip::header_opt!(resp.headers().iter(), Header::ProxyAuthenticate);
            let proxy_header = proxy_header.ok_or(crate::Error::DialogError(
                "missing proxy/www authenticate".to_string(),
            ))?;
            Header::ProxyAuthenticate(proxy_header.clone())
        }
    };

    let mut new_req = tx.original.clone();
    new_req.cseq_header_mut()?.mut_seq(new_seq)?;

    let auth_qop = AuthQop::Auth {
        cnonce: random_text(CNONCE_LEN),
        nc: 1,
    };

    let challenge = match &header {
        Header::WwwAuthenticate(h) => h.typed()?,
        Header::ProxyAuthenticate(h) => h.typed()?.0,
        _ => unreachable!(),
    };

    let response = DigestGenerator {
        username: cred.username.as_str(),
        password: cred.password.as_str(),
        algorithm: challenge.algorithm.unwrap_or_default(),
        nonce: challenge.nonce.as_str(),
        method: &tx.original.method,
        qop: Some(&auth_qop),
        uri: &tx.original.uri,
        realm: challenge.realm.as_str(),
    }
    .compute();

    let auth = Authorization {
        scheme: challenge.scheme,
        username: cred.username.clone(),
        realm: challenge.realm,
        nonce: challenge.nonce,
        uri: tx.original.uri.clone(),
        response,
        algorithm: challenge.algorithm,
        opaque: challenge.opaque,
        qop: Some(auth_qop),
    };

    let via_header = tx.original.via_header()?.clone();

    // update new branch
    let mut params = via_header.params().clone()?;
    params.retain(|k| !matches!(k, Param::Branch(_)));
    params.push(make_via_branch());
    new_req.headers_mut().unique_push(via_header.into());

    new_req.headers_mut().retain(|h| {
        !matches!(
            h,
            Header::ProxyAuthenticate(_)
                | Header::Authorization(_)
                | Header::WwwAuthenticate(_)
                | Header::ProxyAuthorization(_)
        )
    });

    match header {
        Header::WwwAuthenticate(_) => {
            new_req.headers_mut().unique_push(auth.into());
        }
        Header::ProxyAuthenticate(_) => {
            new_req
                .headers_mut()
                .unique_push(ProxyAuthorization(auth).into());
        }
        _ => unreachable!(),
    }

    let new_tx = Transaction::new_client(
        tx.key.clone(),
        new_req,
        tx.endpoint_inner.clone(),
        tx.connection.clone(),
    );
    Ok(new_tx)
}
