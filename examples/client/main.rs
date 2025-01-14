use clap::Parser;
use rsip::typed::MediaType;
use rsipstack::dialog::dialog::{Dialog, DialogState, DialogStateReceiver, DialogStateSender};
use rsipstack::dialog::dialog_layer::DialogLayer;
use rsipstack::dialog::server_dialog::ServerInviteDialog;
use rsipstack::transaction::Endpoint;
use rsipstack::transport::connection::SipAddr;
use rsipstack::Result;
use rsipstack::{
    dialog::{authenticate::Credential, registration::Registration},
    transaction::TransactionReceiver,
    transport::{udp::UdpConnection, TransportLayer},
    EndpointBuilder, Error,
};
use rtp_rs::RtpPacketBuilder;
use std::net::IpAddr;
use std::{env, sync::Arc, time::Duration};
use tokio::sync::mpsc::unbounded_channel;
use tokio::{select, time::sleep};
use tokio_util::sync::CancellationToken;
use tracing::{debug, info};
mod stun;

struct MediaSessionOption {
    pub stun: bool,
    pub stun_server: Option<String>,
    pub external_ip: Option<String>,
    pub rtp_start_port: u16,
}

/// A SIP client example that sends a REGISTER request to a SIP server.
#[derive(Parser, Debug)]
#[command(author, version, about, long_about = None)]
struct Args {
    /// SIP port
    #[arg(long, default_value = "25060")]
    port: u16,

    /// SIP port
    #[arg(long, default_value = "12000")]
    rtp_start_port: u16,

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

    #[arg(long)]
    stun: bool,
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

    let sip_server = args
        .sip_server
        .unwrap_or(env::var("SIP_SERVER").unwrap_or_default());
    let sip_username = args
        .user
        .unwrap_or(env::var("SIP_USERNAME").unwrap_or_default());
    let sip_password = args
        .password
        .unwrap_or(env::var("SIP_PASSWORD").unwrap_or_default());

    let opt = MediaSessionOption {
        stun: args.stun,
        stun_server: args.stun_server.clone(),
        external_ip: args.external_ip.clone(),
        rtp_start_port: args.rtp_start_port,
    };

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

    let addr = get_first_non_loopback_interface()?;
    let mut connection = UdpConnection::create_connection(
        format!("{}:{}", addr, args.port).parse()?,
        external.clone(),
    )
    .await?;

    if external.is_none() && args.stun {
        if let Some(server) = args.stun_server {
            match stun::external_by_stun(&mut connection, &server, Duration::from_secs(5)).await {
                Ok(socket) => info!("external IP: {:?}", socket),
                Err(e) => info!("Failed to get external IP, stunserver {} : {:?}", server, e),
            }
        }
    }

    transport_layer.add_transport(connection.into());

    let endpoint = Arc::new(
        EndpointBuilder::new()
            .cancel_token(token)
            .transport_layer(transport_layer)
            .build(),
    );

    let credential = Credential {
        username: sip_username.clone(),
        password: sip_password,
    };

    let endpoint_ref = endpoint.clone();
    let incoming = endpoint_ref.incoming_transactions();
    let dialog_layer = Arc::new(DialogLayer::new(endpoint_ref.inner.clone()));

    let (state_sender, state_receiver) = unbounded_channel();

    let first_addr = endpoint
        .get_addrs()
        .first()
        .ok_or(crate::Error::Error("no address found".to_string()))?
        .clone();

    let contact = rsip::Uri {
        scheme: Some(rsip::Scheme::Sip),
        auth: Some(rsip::Auth {
            user: sip_username,
            password: None,
        }),
        host_with_port: first_addr.addr.into(),
        params: vec![],
        headers: vec![],
    };

    select! {
        _ = endpoint.serve() => {
            info!("user agent finished");
        }
        r = process_registration(endpoint_ref, sip_server, credential) => {
            info!("register loop finished {:?}", r);
        }
        r = process_incoming_request(dialog_layer.clone(), incoming, state_sender, contact) => {
            info!("serve loop finished {:?}", r);
        }
        r = process_dialog(dialog_layer, state_receiver, opt) => {
            info!("dialog loop finished {:?}", r);
        }
    }
    Ok(())
}

async fn process_registration(
    endpoint: Arc<Endpoint>,
    sip_server: String,
    credential: Credential,
) -> Result<()> {
    if sip_server.is_empty() {
        loop {
            sleep(Duration::from_secs(1)).await;
        }
    }
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

async fn process_incoming_request(
    dialog_layer: Arc<DialogLayer>,
    mut incoming: TransactionReceiver,
    state_sender: DialogStateSender,
    contact: rsip::Uri,
) -> Result<()> {
    while let Some(mut tx) = incoming.recv().await {
        info!("Received transaction: {:?}", tx.key);
        match tx.original.method {
            rsip::Method::Invite | rsip::Method::Ack | rsip::Method::Bye => {
                let mut dialog = match dialog_layer.get_or_create_server_invite(
                    &tx,
                    state_sender.clone(),
                    None,
                    Some(contact.clone()),
                ) {
                    Ok(d) => d,
                    Err(e) => {
                        // 481 Dialog/Transaction Does Not Exist
                        info!("Failed to obtain dialog: {:?}", e);
                        tx.reply(rsip::StatusCode::CallTransactionDoesNotExist)
                            .await?;
                        continue;
                    }
                };
                tokio::spawn(async move {
                    dialog.handle(tx).await?;
                    Ok::<_, Error>(())
                });
            }
            _ => {
                info!("Received request: {:?}", tx.original.method);
                tx.reply(rsip::StatusCode::OK).await?;
            }
        }
    }
    Ok::<_, Error>(())
}

async fn process_dialog(
    dialog_layer: Arc<DialogLayer>,
    state_receiver: DialogStateReceiver,
    opt: MediaSessionOption,
) -> Result<()> {
    let mut state_receiver = state_receiver;
    while let Some(state) = state_receiver.recv().await {
        match state {
            DialogState::Calling(id) => {
                info!("Calling dialog {}", id);
                let dialog = match dialog_layer.get_dialog(&id) {
                    Some(d) => d,
                    None => {
                        info!("Dialog not found {}", id);
                        continue;
                    }
                };
                match dialog {
                    Dialog::ServerInvite(d) => {
                        // play example pcmu of handling incoming call
                        //
                        // [A] Ai answer, [R] Reject, [E] Play example pcmu
                        play_example_pcmu(&opt, d).await?;
                    }
                    _ => {}
                }
            }
            DialogState::Early(id, resp) => {
                info!("Early dialog {} {}", id, resp);
            }
            DialogState::Terminated(id, status_code) => {
                info!("Dialog terminated {} {:?}", id, status_code);
                dialog_layer.remove_dialog(&id);
            }
            _ => {
                info!("Received dialog state: {}", state);
            }
        }
    }
    Ok(())
}

fn get_first_non_loopback_interface() -> Result<IpAddr> {
    for iface in pnet::datalink::interfaces() {
        if !iface.is_loopback() {
            for ip in iface.ips {
                if let IpAddr::V4(_) = ip.ip() {
                    return Ok(ip.ip());
                }
            }
        }
    }
    Err(Error::Error("No interface found".to_string()))
}

async fn play_example_pcmu(opt: &MediaSessionOption, dialog: ServerInviteDialog) -> Result<()> {
    let example_data = tokio::fs::read("./assets/example.pcmu").await?;
    let addr = get_first_non_loopback_interface()?;
    let mut conn = None;
    info!("Non loopback interface: {:?}", addr);

    for p in 0..100 {
        let port = opt.rtp_start_port + p;
        conn = match UdpConnection::create_connection(format!("{:?}:{}", addr, port).parse()?, None)
            .await
        {
            Ok(c) => Some(c),
            Err(e) => {
                info!("Failed to bind RTP socket: {:?}", e);
                None
            }
        };
        if conn.is_some() {
            break;
        }
    }

    if conn.is_none() {
        return Err(Error::Error("Failed to bind RTP socket".to_string()));
    }

    let mut conn = conn.unwrap();
    if opt.external_ip.is_none() && opt.stun {
        if let Some(ref server) = opt.stun_server {
            match stun::external_by_stun(&mut conn, &server, Duration::from_secs(5)).await {
                Ok(socket) => info!("external IP: {:?}", socket),
                Err(e) => info!("Failed to get external IP, stunserver {} : {:?}", server, e),
            }
        }
    }
    info!("RTP socket: {:?}", conn.get_addr());
    let ssrc = rand::random::<u32>();
    let answer = format!(
        "v=0\r\n\
        o=- 0 0 IN IP4 {}\r\n\
        s=rsipstack example\r\n\
        c=IN IP4 {}\r\n\
        t=0 0\r\n\
        m=audio {} RTP/AVP 0\r\n\
        a=rtpmap:0 PCMU/8000\r\n\
        a=ssrc:{}\r\n\
        a=sendrecv\r\n",
        conn.get_addr().addr.ip(),
        conn.get_addr().addr.ip(),
        conn.get_addr().addr.port(),
        ssrc,
    );

    let headers = vec![rsip::typed::ContentType(MediaType::Sdp(vec![])).into()];
    dialog.accept(Some(headers), Some(answer.into()))?;
    let rtp_token = dialog.cancel_token().child_token();
    let body = String::from_utf8_lossy(dialog.initial_request().body()).to_string();
    let offer = match sdp_rs::SessionDescription::try_from(body.as_str()) {
        Ok(s) => s,
        Err(e) => {
            info!("Failed to parse offer SDP: {:?} {}", e, body);
            return Err(Error::Error("Failed to parse SDP".to_string()));
        }
    };
    let peer_addr = match offer.connection {
        Some(c) => c.connection_address.base,
        None => {
            info!("No connection address in offer SDP");
            return Err(Error::Error(
                "No connection address in offer SDP".to_string(),
            ));
        }
    };

    let peer_port = offer
        .media_descriptions
        .first()
        .map(|m| m.media.port)
        .ok_or(Error::Error("No audio port in offer SDP".to_string()))?;

    let peer_addr = format!("{}:{}", peer_addr, peer_port);

    tokio::spawn(async move {
        select! {
            _ = rtp_token.cancelled() => {
                info!("RTP session cancelled");
            }
            _ = async {
                let peer_addr = SipAddr{
                    addr: peer_addr.parse().unwrap(),
                    r#type: Some(rsip::transport::Transport::Udp),
                };
                let mut ts = 0;
                let sample_size = 160;
                let mut seq = 1;
                let mut ticker = tokio::time::interval(Duration::from_millis(20));
                for chunk in example_data.chunks(sample_size) {
                    let result = match RtpPacketBuilder::new()
                    .payload_type(0)
                    .ssrc(ssrc)
                    .sequence(seq.into())
                    .timestamp(ts)
                    .payload(&chunk)
                    .build() {
                        Ok(r) => r,
                        Err(e) => {
                            info!("Failed to build RTP packet: {:?}", e);
                            break;
                        }
                    };
                    ts += chunk.len() as u32;
                    seq += 1;
                    match conn.send_raw(&result, &peer_addr).await {
                        Ok(_) => {},
                        Err(e) => {
                            info!("Failed to send RTP: {:?}", e);
                            break;
                        }
                    }
                    ticker.tick().await;
                    // if seq > 100 {
                    //     break;
                    // }
                }
            } => {
                info!("playback finished, hangup");
                match dialog.bye().await {
                    Ok(_) => {},
                    Err(e) => {
                        info!("Failed to send BYE: {:?}", e);
                    }
                }
            }
        };
        Ok::<_, Error>(())
    });

    Ok(())
}
