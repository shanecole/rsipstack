use clap::Parser;
use rsipstack::dialog::dialog_layer::DialogLayer;
use rsipstack::dialog::DialogId;
use rsipstack::transaction::{endpoint, Endpoint};
use rsipstack::Result;
use rsipstack::{
    dialog::{authenticate::Credential, registration::Registration},
    transaction::TransactionReceiver,
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

    #[arg(long, default_value = "restsend.com:3478")]
    stun_server: Option<String>,
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

    let external_ip = args
        .external_ip
        .unwrap_or(env::var("EXTERNAL_IP").unwrap_or_default());

    let external = if external_ip.is_empty() {
        None
    } else {
        Some(format!("{}:{}", external_ip, args.port).parse()?)
    };

    let mut connection = UdpConnection::create_connection(
        format!("0.0.0.0:{}", args.port).parse()?,
        external.clone(),
    )
    .await?;

    if external.is_none() && args.stun_server.is_some() {
        connection
            .external_by_stun(args.stun_server.unwrap())
            .await?;
    }

    transport_layer.add_transport(connection.into());

    let endpoint = Arc::new(
        EndpointBuilder::new()
            .cancel_token(token)
            .transport_layer(transport_layer)
            .build(),
    );

    let credential = Credential {
        username: sip_username,
        password: sip_password,
    };

    let endpoint_ref = endpoint.clone();
    let incoming = endpoint_ref.incoming_transactions();
    let dialog_layer = DialogLayer::new(endpoint_ref.inner.clone());
    select! {
        _ = endpoint.serve() => {
            info!("user agent finished");
        }
        r = process_registration(endpoint_ref.clone(), sip_server, credential) => {
            info!("register loop finished {:?}", r);
        }
        r = process_incoming(dialog_layer, incoming) => {
            info!("serve loop finished {:?}", r);
        }
    }
    Ok(())
}

async fn process_registration(
    endpoint: Arc<Endpoint>,
    sip_server: String,
    credential: Credential,
) -> Result<()> {
    let mut registration = Registration::new(endpoint, Some(credential));
    loop {
        let resp = registration.register(&sip_server).await?;
        debug!("received response: {:?}", resp);
        if resp.status_code != rsip::StatusCode::OK {
            info!("Failed to register: {:?}", resp);
            return Err(rsipstack::Error::Error("Failed to register".to_string()));
        }
        let expires = registration.expires();
        sleep(Duration::from_secs(expires as u64)).await;
    }
    #[allow(unreachable_code)]
    Ok::<_, Error>(())
}

async fn process_incoming(
    dialog_layer: DialogLayer,
    mut incoming: TransactionReceiver,
) -> Result<()> {
    while let Some(mut tx) = incoming.recv().await {
        info!("Received transaction: {:?}", tx.key);
        match tx.original.method {
            rsip::Method::Invite => {
                let dialog_id = match DialogId::try_from(&tx.original) {
                    Ok(dialog_id) => dialog_id,
                    Err(e) => {
                        info!("Failed to create dialog id, tx{:?}  {:?}", tx.original, e);
                        continue;
                    }
                };

                let dialog = dialog_layer.create_or_create_server_invite(tx)?;
                dialog.handle(tx).await?;
            }
            _ => {
                let resp =
                    tx.endpoint_inner
                        .make_response(&tx.original, rsip::StatusCode::OK, None);
                info!("Received request: {:?}", tx.original.method);
                tx.respond(resp).await?;
            }
        }
    }
    Ok::<_, Error>(())
}
