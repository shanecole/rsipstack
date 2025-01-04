use tokio_util::sync::CancellationToken;

use crate::transport::TransportLayer;

use super::{endpoint::Endpoint, EndpointBuilder};

mod test_client;

pub(super) fn create_test_endpoint() -> Endpoint {
    let token = CancellationToken::new();
    let tl = TransportLayer::new(token.child_token());
    EndpointBuilder::new()
        .user_agent("rsipstack-test")
        .transport_layer(tl)
        .build()
}
