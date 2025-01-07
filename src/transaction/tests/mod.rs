use tokio_util::sync::CancellationToken;

use crate::{
    transport::{udp::UdpConnection, TransportLayer},
    Result,
};

use super::{endpoint::Endpoint, EndpointBuilder};

mod test_server;
mod test_endpoint;

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
