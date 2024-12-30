use rsipstack::EndpointBuilder;

#[tokio::main]
async fn main() {
    tracing_subscriber::fmt()
        .with_file(true)
        .with_line_number(true)
        .try_init()
        .ok();
    let server = EndpointBuilder::new().build();
    todo!()
}
