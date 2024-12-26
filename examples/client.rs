use rsip::headers::to;
use rsipstack::EndpointBuilder;
use tracing::info;

// A sip client example, that sends a REGISTER request to a sip server.
#[tokio::main]
async fn main() -> rsipstack::Result<()> {
    let user_agent = EndpointBuilder::new().build();
    // let register_req = rsip::message::Request{
    //     method: rsip::method::Method::Register,
    //     uri: rsip::Uri {
    //         scheme: Some(rsip::Scheme::Sips),
    //         host_with_port: rsip::Domain::from("example.com").into(),
    //         ..Default::default()
    //     },
    //     headers: rsip::Headers::default(),
    //     version: rsip::Version::V2,
    //     body: Default::default(),
    // };

    // let tx = endpoint.client_transaction(register_req)?;
    // tx.send().await.expect("Failed to send register request.");
    // while let Ok(resp) = tx.receive().await {
    //     info!("Received response: {:?}", resp);
    // }
    // Ok(())
    todo!()
}
