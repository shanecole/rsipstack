use rsipstack::{EndpointBuilder, Error};
use std::sync::Arc;
use tokio::select;
use tracing::info;

// A sip client example, that sends a REGISTER request to a sip server.
#[tokio::main]
async fn main() -> rsipstack::Result<()> {
    let user_agent = Arc::new(EndpointBuilder::new().build());
    let user_agent_ref = user_agent.clone();

    tokio::spawn(async move {
        let register_req = rsip::message::Request {
            method: rsip::method::Method::Register,
            uri: rsip::Uri {
                scheme: Some(rsip::Scheme::Sips),
                host_with_port: rsip::Domain::from("example.com").into(),
                ..Default::default()
            },
            headers: rsip::Headers::default(),
            version: rsip::Version::V2,
            body: Default::default(),
        };
        let mut tx = user_agent_ref.client_transaction(register_req)?;
        tx.send().await?;

        while let Some(resp) = tx.receive().await {
            info!("Received response: {:?}", resp);
        }
        Ok::<_, Error>(())
    });

    select! {
        _ = user_agent.serve() => {}
    }
    Ok(())
}
