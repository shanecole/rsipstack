use super::endpoint::EndpointInner;
use rsip::{
    headers::{CallId, UserAgent},
    prelude::UntypedHeader,
    Header, Request, Response, StatusCode,
};

impl EndpointInner {
    pub fn make_request(
        &self,
        method: rsip::Method,
        req_uri: rsip::Uri,
        via: rsip::typed::Via,
        from: rsip::typed::From,
        to: rsip::typed::To,
        seq: u32,
    ) -> rsip::Request {
        rsip::Request {
            method,
            uri: req_uri,
            headers: vec![
                Header::Via(via.into()),
                Header::CallId(CallId::default().into()),
                Header::From(from.into()),
                Header::To(to.into()),
                Header::CSeq(rsip::typed::CSeq { seq, method }.into()),
                Header::MaxForwards(70.into()),
                Header::UserAgent(self.user_agent.clone().into()),
            ]
            .into(),
            body: vec![],
            version: rsip::Version::V2,
        }
    }

    pub fn make_response(
        &self,
        req: &Request,
        status: StatusCode,
        body: Option<Vec<u8>>,
    ) -> Response {
        let mut headers = req.headers.clone();
        headers.unique_push(UserAgent::new(self.user_agent.clone()).into());

        Response {
            status_code: status,
            version: req.version().clone(),
            headers,
            body: body.unwrap_or_default(),
        }
    }
}
