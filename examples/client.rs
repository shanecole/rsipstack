use clap::Parser;
use rsip::prelude::HeadersExt;
use rsipstack::{
    dialog::{authenticate::Credential, registration::Registration},
    transport::{udp::UdpConnection, TransportLayer},
    EndpointBuilder, Error,
};
use std::{env, sync::Arc, time::Duration};
use tokio::{select, time::sleep};
use tokio_util::sync::CancellationToken;
use tracing::{debug, info};

/// A SIP client example that sends a REGISTER request to a SIP server.
#[derive(Parser, Debug)]
#[command(author, version, about, long_about = None)]
struct Args {
    /// SIP port
    #[arg(long, default_value = "25060")]
    port: u16,

    /// External IP address
    #[arg(long, default_value = "")]
    external_ip: Option<String>,

    /// SIP server address
    #[arg(long)]
    sip_server: Option<String>,

    /// SIP user
    #[arg(long)]
    user: Option<String>,

    /// SIP password
    #[arg(long)]
    password: Option<String>,
}

// A sip client example, that sends a REGISTER request to a sip server.
#[tokio::main]
async fn main() -> rsipstack::Result<()> {
    tracing_subscriber::fmt()
        .with_max_level(tracing::Level::TRACE)
        .with_file(true)
        .with_line_number(true)
        .with_timer(tracing_subscriber::fmt::time::LocalTime::rfc_3339())
        .try_init()
        .ok();

    if let Err(e) = dotenv::dotenv() {
        info!("Failed to load .env file: {}", e);
    }

    let args = Args::parse();

    info!("Starting SIP client");
    let sip_server = args.sip_server.unwrap_or(env::var("SIP_SERVER")?);
    let sip_username = args.user.unwrap_or(env::var("SIP_USERNAME")?);
    let sip_password = args.password.unwrap_or(env::var("SIP_PASSWORD")?);

    let token = CancellationToken::new();
    let transport_layer = TransportLayer::new(token.clone());

    let connection =
        UdpConnection::create_connection(format!("0.0.0.0:{}", args.port).parse()?, None).await?;
    transport_layer.add_transport(connection.into());

    let user_agent = Arc::new(
        EndpointBuilder::new()
            .cancel_token(token)
            .transport_layer(transport_layer)
            .build(),
    );

    let user_agent_ref = user_agent.clone();

    let register_loop = async {
        let auth_option = Credential {
            username: sip_username,
            password: sip_password,
        };

        let mut registration = Registration::new(user_agent_ref, Some(auth_option));

        loop {
            let resp = registration.register(&sip_server).await?;
            debug!("received response: {:?}", resp);
            if resp.status_code != rsip::StatusCode::OK {
                info!("Failed to register: {:?}", resp);
                return Err(rsipstack::Error::Error("Failed to register".to_string()));
            }
            let expires = resp.expires_header().unwrap_or(&50.into()).seconds()?;
            sleep(Duration::from_secs(expires as u64)).await;
        }
        Ok::<_, Error>(())
    };
    let user_agent_ref = user_agent.clone();
    let mut incoming = user_agent_ref.incoming_transactions();

    let serve_loop = async move {
        while let Some(mut tx) = incoming.recv().await {
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
        _ = user_agent.serve() => {
            info!("User agent finished");
        }
        r = register_loop => {
            info!("Register loop finished {:?}", r);
        }
        _ = serve_loop => {
            info!("Server loop finished");
        }
    }
    Ok(())
}
