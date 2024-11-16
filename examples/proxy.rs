use rsipstack::EndpointBuilder;

#[tokio::main]
async fn main() {
    let user_agent = EndpointBuilder::new().build();
    
    user_agent.serve().await;
}