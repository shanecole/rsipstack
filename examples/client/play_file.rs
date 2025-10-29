use crate::{MediaSessionOption, get_first_non_loopback_interface};
use rsipstack::Result;
use rsipstack::transport::udp::UdpConnection;
use rsipstack::{Error, transport::SipAddr};
use rtp_rs::RtpPacketBuilder;
use std::net::SocketAddr;
use std::time::Duration;
use tokio::select;
use tokio_util::sync::CancellationToken;
use tracing::info;

pub async fn build_rtp_conn(
    opt: &MediaSessionOption,
    ssrc: u32,
    payload_type: u8,
) -> Result<(UdpConnection, String)> {
    let addr = get_first_non_loopback_interface()?;
    let mut conn = None;

    for p in 0..100 {
        let port = opt.rtp_start_port + p * 2;
        if let Ok(c) = UdpConnection::create_connection(
            format!("{:?}:{}", addr, port).parse()?,
            opt.external_ip
                .as_ref()
                .map(|ip| ip.parse::<SocketAddr>().expect("Invalid external IP")),
            Some(opt.cancel_token.clone()),
        )
        .await
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

    let conn = conn.unwrap();
    let codec = payload_type;
    let codec_name = match codec {
        0 => "PCMU",
        8 => "PCMA",
        _ => "Unknown",
    };
    let socketaddr: SocketAddr = conn.get_addr().addr.to_owned().try_into()?;
    let sdp = format!(
        "v=0\r\n\
        o=- 0 0 IN IP4 {}\r\n\
        s=rsipstack example\r\n\
        c=IN IP4 {}\r\n\
        t=0 0\r\n\
        m=audio {} RTP/AVP {codec}\r\n\
        a=rtpmap:{codec} {codec_name}/8000\r\n\
        a=ssrc:{ssrc}\r\n\
        a=sendrecv\r\n",
        socketaddr.ip(),
        socketaddr.ip(),
        socketaddr.port(),
    );
    info!("RTP socket: {:?} {}", conn.get_addr(), sdp);
    Ok((conn, sdp))
}

pub async fn play_echo(conn: UdpConnection, token: CancellationToken) -> Result<()> {
    select! {
        _ = token.cancelled() => {
            info!("RTP session cancelled");
        }
        _ = async {
            loop {
                let mut mbuf = vec![0; 1500];
                let (len, addr) = match conn.recv_raw(&mut mbuf).await {
                    Ok(r) => r,
                    Err(e) => {
                        info!("Failed to receive RTP: {:?}", e);
                        break;
                    }
                };
                match conn.send_raw(&mbuf[..len], &addr).await {
                    Ok(_) => {},
                    Err(e) => {
                        info!("Failed to send RTP: {:?}", e);
                        break;
                    }
                }
            }
        } => {
            info!("playback finished, hangup");
        }
    };
    Ok(())
}

#[allow(clippy::too_many_arguments)]
pub async fn play_audio_file(
    conn: UdpConnection,
    token: CancellationToken,
    ssrc: u32,
    filename: &str,
    mut ts: u32,
    mut seq: u16,
    peer_addr: String,
    payload_type: u8,
) -> Result<(u32, u16)> {
    select! {
        _ = token.cancelled() => {
            info!("RTP session cancelled");
        }
        _ = async {
            let peer_addr = SipAddr{
                addr: peer_addr.try_into().expect("peer_addr"),
                r#type: Some(rsip::transport::Transport::Udp),
            };
            let sample_size = 160;
            let mut ticker = tokio::time::interval(Duration::from_millis(20));
            let ext = match payload_type {
                8 => "pcma",
                0 => "pcmu",
                _ => {
                    info!("Unsupported codec type: {}", payload_type);
                    return;
                }
            };
            let file_name = format!("./assets/{filename}.{ext}");
            info!("Playing {filename} file: {} payload_type:{} sample_size:{}", file_name, payload_type, sample_size);
            let example_data = tokio::fs::read(file_name).await.expect("read file");

            for chunk in example_data.chunks(sample_size) {
                let result = match RtpPacketBuilder::new()
                .payload_type(payload_type)
                .ssrc(ssrc)
                .sequence(seq.into())
                .timestamp(ts)
                .payload(chunk)
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
    Ok((ts, seq))
}
