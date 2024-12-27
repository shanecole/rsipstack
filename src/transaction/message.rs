use rsip::{Request, Response, StatusCode};

pub(crate) fn make_response(req: &Request, status: StatusCode, body: Option<Vec<u8>>) -> Response {
    Response {
        status_code: status,
        version: req.version().clone(),
        headers: req.headers.clone(),
        body: body.unwrap_or_default(),
    }
}
