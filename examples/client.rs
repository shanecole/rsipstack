use rsipstack::{
    transport::{udp::UdpConnection, TransportLayer},
    EndpointBuilder, Error,
};
use std::sync::Arc;
use tokio::select;
use tokio_util::sync::CancellationToken;
use tracing::info;

// A sip client example, that sends a REGISTER request to a sip server.
#[tokio::main]
async fn main() -> rsipstack::Result<()> {
    use rsip::headers::*;
    tracing_subscriber::fmt()
        .with_max_level(tracing::Level::TRACE)
        .with_file(true)
        .with_line_number(true)
        .with_timer(tracing_subscriber::fmt::time::LocalTime::rfc_3339())
        .try_init()
        .ok();

    let token = CancellationToken::new();
    let transport_layer = TransportLayer::new(token.clone());

    let connection = UdpConnection::create_connection("0.0.0.0:15060".parse()?, None).await?;
    transport_layer.add_transport(connection.into());

    let user_agent = Arc::new(
        EndpointBuilder::new()
            .cancel_token(token)
            .transport_layer(transport_layer)
            .build(),
    );

    let user_agent_ref = user_agent.clone();

    tokio::spawn(async move {
        let register_loop = async {
            let register_req = rsip::message::Request {
                method: rsip::method::Method::Register,
                uri: rsip::Uri {
                    scheme: Some(rsip::Scheme::Sip),
                    host_with_port: rsip::HostWithPort::try_from("127.0.0.1:8880")?.into(),
                    ..Default::default()
                },
                headers: vec![
                    Via::new("SIP/2.0/TLS restsend.com:5061;branch=z9hG4bKnashd92").into(),
                    CSeq::new("1 REGISTER").into(),
                    From::new("Bob <sips:bob@restsend.com>;tag=ja743ks76zlflH").into(),
                    CallId::new("1j9FpLxk3uxtm8tn@restsend.com").into(),
                ]
                .into(),
                version: rsip::Version::V2,
                body: Default::default(),
            };
            let mut tx = user_agent_ref.client_transaction(register_req)?;
            match tx.send().await {
                Ok(_) => info!("Sent request"),
                Err(e) => info!("Error sending request: {:?}", e),
            }
            while let Some(resp) = tx.receive().await {
                info!("Received response: {:?}", resp);
            }
            Ok::<_, Error>(())
        };
        match register_loop.await {
            Ok(_) => info!("Register loop done"),
            Err(e) => info!("Error in register loop: {:?}", e),
        }
    });

    let user_agent_ref = user_agent.clone();
    let mut incoming = user_agent_ref.incoming_transactions();

    let serve_loop = async move {
        while let Some(Some(mut tx)) = incoming.recv().await {
            info!("Received transaction: {:?}", tx.key);
            while let Some(msg) = tx.receive().await {
                info!("Received message: {:?}", msg);
                let done_response = rsip::Response {
                    status_code: rsip::StatusCode::NotAcceptable,
                    version: rsip::Version::V2,
                    ..Default::default()
                };
                tx.respond(done_response).await?;
            }
        }
        Ok::<_, Error>(())
    };

    select! {
        _ = user_agent.serve() => {}
        _ = serve_loop => {}
    }
    Ok(())
}
