use std::{sync::Arc, time::Duration};

use rsipstack::EndpointBuilder;
use tokio::{spawn, time::sleep};

#[tokio::test]
async fn test_endpoint() {
    tracing_subscriber::fmt()
        .with_file(true)
        .with_line_number(true)
        .try_init()
        .ok();
    let endpoint = Arc::new(EndpointBuilder::new().build());
    let endpoint_ref = endpoint.clone();

    spawn(async move {
        endpoint.serve().await;
    });

    sleep(Duration::from_millis(10)).await;
    endpoint_ref.shutdown();
    sleep(Duration::from_millis(10)).await;
}
