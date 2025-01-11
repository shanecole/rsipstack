use tokio_util::sync::CancellationToken;

use crate::{
    transport::{udp::UdpConnection, TransportLayer},
    Result,
};

use super::{endpoint::Endpoint, EndpointBuilder};

mod test_client;
mod test_endpoint;
mod test_server;

pub(super) async fn create_test_endpoint(addr: Option<&str>) -> Result<Endpoint> {
    let token = CancellationToken::new();
    let tl = TransportLayer::new(token.child_token());

    if let Some(addr) = addr {
        let peer = UdpConnection::create_connection(addr.parse()?, None).await?;
        tl.add_transport(peer.into());
    }

    let endpoint = EndpointBuilder::new()
        .user_agent("rsipstack-test")
        .transport_layer(tl)
        .build();
    Ok(endpoint)
}

#[test]
fn test_random_text() {
    let text = super::random_text(10);
    assert_eq!(text.len(), 10);
    let branch = super::make_via_branch();
    let branch = branch.to_string();
    assert_eq!(branch.len(), 27); // ;branch=z9hG4bK
}
