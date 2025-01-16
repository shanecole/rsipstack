use std::time::Duration;

use rsipstack::transport::udp::UdpConnection;
use rsipstack::Result;
use rsipstack::{transport::connection::SipAddr, Error};
use rtp_rs::RtpPacketBuilder;
use tokio::select;
use tokio_util::sync::CancellationToken;
use tracing::info;

use crate::{stun, MediaSessionOption};

pub async fn build_rtp_conn(
    opt: &MediaSessionOption,
    ssrc: u32,
) -> Result<(UdpConnection, String)> {
    let addr = stun::get_first_non_loopback_interface()?;
    let mut conn = None;

    for p in 0..100 {
        let port = opt.rtp_start_port + p * 2;
        if let Ok(c) =
            UdpConnection::create_connection(format!("{:?}:{}", addr, port).parse()?, None).await
        {
            conn = Some(c);
            break;
        } else {
            info!("Failed to bind RTP socket on port: {}", port);
        }
    }

    if conn.is_none() {
        return Err(Error::Error("Failed to bind RTP socket".to_string()));
    }

    let mut conn = conn.unwrap();
    if opt.external_ip.is_none() && opt.stun {
        if let Some(ref server) = opt.stun_server {
            match stun::external_by_stun(&mut conn, &server, Duration::from_secs(5)).await {
                Ok(socket) => info!("media external IP by stun: {:?}", socket),
                Err(e) => info!(
                    "Failed to get media external IP, stunserver {} : {:?}",
                    server, e
                ),
            }
        }
    }
    let sdp = format!(
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
    info!("RTP socket: {:?} {}", conn.get_addr(), sdp);
    Ok((conn, sdp))
}

pub async fn play_example_file(
    conn: UdpConnection,
    token: CancellationToken,
    ssrc: u32,
    peer_addr: String,
) -> Result<()> {
    select! {
        _ = token.cancelled() => {
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

            let example_data = tokio::fs::read("./assets/example.pcmu").await.expect("read example.pcmu");

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
            }
        } => {
            info!("playback finished, hangup");
        }
    };
    Ok(())
}
