use rsip::{headers::UserAgent, prelude::UntypedHeader, Request, Response, StatusCode};

use super::transaction::TransactionCore;

impl TransactionCore {
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
