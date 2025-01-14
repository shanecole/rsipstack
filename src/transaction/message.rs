use super::{endpoint::EndpointInner, make_call_id};
use rsip::{Header, Request, Response, StatusCode};

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
        let headers = vec![
            Header::Via(via.into()),
            Header::CallId(make_call_id(None)),
            Header::From(from.into()),
            Header::To(to.into()),
            Header::CSeq(rsip::typed::CSeq { seq, method }.into()),
            Header::MaxForwards(70.into()),
            Header::UserAgent(self.user_agent.clone().into()),
        ];
        rsip::Request {
            method,
            uri: req_uri,
            headers: headers.into(),
            body: vec![],
            version: rsip::Version::V2,
        }
    }

    pub fn make_response(
        &self,
        req: &Request,
        status_code: StatusCode,
        body: Option<Vec<u8>>,
    ) -> Response {
        let mut headers = req.headers.clone();
        headers.retain(|h| {
            matches!(
                h,
                Header::Via(_)
                    | Header::CallId(_)
                    | Header::From(_)
                    | Header::To(_)
                    | Header::MaxForwards(_)
                    | Header::CSeq(_)
            )
        });
        headers.unique_push(Header::UserAgent(self.user_agent.clone().into()));
        Response {
            status_code,
            version: req.version().clone(),
            headers,
            body: body.unwrap_or_default(),
        }
    }
}
