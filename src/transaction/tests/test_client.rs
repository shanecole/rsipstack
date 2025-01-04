#[tokio::test]
async fn test_client_transaction() {
    tracing_subscriber::fmt()
        .with_file(true)
        .with_line_number(true)
        .try_init()
        .ok();
    let endpoint = super::create_test_endpoint();
}
