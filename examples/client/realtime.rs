use base64::engine::general_purpose;
use base64::Engine;
use dasp::interpolate::sinc::Sinc;
use dasp::sample::ToSample;
use dasp::{frame, ring_buffer, Signal};
use futures::{SinkExt, StreamExt};
use openai_api_rs::realtime::api::RealtimeClient;
use openai_api_rs::realtime::client_event::{InputAudioBufferAppend, SessionUpdate};
use openai_api_rs::realtime::server_event::ServerEvent;
use rsipstack::transport::connection::SipAddr;
use rsipstack::transport::udp::UdpConnection;
use rsipstack::Result;
use rtp_rs::RtpPacketBuilder;
use std::sync::{Arc, Mutex};
use tokio::select;
use tokio_tungstenite::connect_async;
use tokio_tungstenite::tungstenite::client::IntoClientRequest;
use tokio_tungstenite::tungstenite::protocol::Message;
use tokio_util::sync::CancellationToken;
use tracing::info;

const SAMPLE_SIZE: usize = 160;
const PCMU_TO_PCM16: [i16; 256] = [
    -32124, -31100, -30076, -29052, -28028, -27004, -25980, -24956, -23932, -22908, -21884, -20860,
    -19836, -18812, -17788, -16764, -15996, -15484, -14972, -14460, -13948, -13436, -12924, -12412,
    -11900, -11388, -10876, -10364, -9852, -9340, -8828, -8316, -7932, -7676, -7420, -7164, -6908,
    -6652, -6396, -6140, -5884, -5628, -5372, -5116, -4860, -4604, -4348, -4092, -3900, -3772,
    -3644, -3516, -3388, -3260, -3132, -3004, -2876, -2748, -2620, -2492, -2364, -2236, -2108,
    -1980, -1884, -1820, -1756, -1692, -1628, -1564, -1500, -1436, -1372, -1308, -1244, -1180,
    -1116, -1052, -988, -924, -876, -844, -812, -780, -748, -716, -684, -652, -620, -588, -556,
    -524, -492, -460, -428, -396, -372, -356, -340, -324, -308, -292, -276, -260, -244, -228, -212,
    -196, -180, -164, -148, -132, -120, -112, -104, -96, -88, -80, -72, -64, -56, -48, -40, -32,
    -24, -16, -8, 0, 32124, 31100, 30076, 29052, 28028, 27004, 25980, 24956, 23932, 22908, 21884,
    20860, 19836, 18812, 17788, 16764, 15996, 15484, 14972, 14460, 13948, 13436, 12924, 12412,
    11900, 11388, 10876, 10364, 9852, 9340, 8828, 8316, 7932, 7676, 7420, 7164, 6908, 6652, 6396,
    6140, 5884, 5628, 5372, 5116, 4860, 4604, 4348, 4092, 3900, 3772, 3644, 3516, 3388, 3260, 3132,
    3004, 2876, 2748, 2620, 2492, 2364, 2236, 2108, 1980, 1884, 1820, 1756, 1692, 1628, 1564, 1500,
    1436, 1372, 1308, 1244, 1180, 1116, 1052, 988, 924, 876, 844, 812, 780, 748, 716, 684, 652,
    620, 588, 556, 524, 492, 460, 428, 396, 372, 356, 340, 324, 308, 292, 276, 260, 244, 228, 212,
    196, 180, 164, 148, 132, 120, 112, 104, 96, 88, 80, 72, 64, 56, 48, 40, 32, 24, 16, 8, 0,
];
use dasp::Sample;

fn pcmu_to_pcm16_u8(pcmu: &[u8]) -> Vec<u8> {
    let pcm16_samples: Vec<f32> = pcmu
        .iter()
        .map(|&sample| PCMU_TO_PCM16[sample as usize])
        .map(f32::from_sample)
        .collect();

    let frame = dasp::signal::from_interleaved_samples_iter(pcm16_samples);
    let ring_buffer = ring_buffer::Fixed::from([[0.0]; 100]);
    let sinc = Sinc::new(ring_buffer);
    let new_signal = frame.from_hz_to_hz(sinc, 8000 as f64, 24000 as f64);
    let mut pcm16_bytes: Vec<u8> = Vec::new();
    for frame in new_signal.until_exhausted() {
        let sample = frame[0].to_sample::<i16>();
        pcm16_bytes.extend_from_slice(sample.to_le_bytes().as_ref());
    }
    pcm16_bytes
}

pub async fn bridge_realtime(
    realtime_token: String,
    realtime_endpoint: String,
    prompt: String,
    conn: UdpConnection,
    cancel_token: CancellationToken,
    ssrc: u32,
    peer_addr: String,
) -> Result<()> {
    if realtime_token.is_empty() || realtime_endpoint.is_empty() {
        return Err(rsipstack::Error::Error(
            "realtime token or endpoint is empty".to_string(),
        ));
    }
    info!("realtime_endpoint: {}", realtime_endpoint);
    // OpenAI GPT-4 model

    let (mut write, mut read) = if std::env::var("AZURE_API").is_err() {
        let model = "gpt-4o-realtime-preview-2024-10-01".to_string();
        let realtime_client = RealtimeClient::new(realtime_token.clone(), model);
        match realtime_client
            .connect()
            .await
            .map_err(|e| rsipstack::Error::Error(e.to_string()))
        {
            Ok(r) => r,
            Err(e) => {
                info!(
                "Failed to connect to OpenAI Realtime: {:?} {realtime_token:} {realtime_endpoint:?}",
                e
            );
                return Err(e);
            }
        }
    } else {
        let mut request = realtime_endpoint
            .clone()
            .into_client_request()
            .map_err(|e| rsipstack::Error::Error(e.to_string()))?;
        request
            .headers_mut()
            .insert("api-key", format!("{realtime_token}").parse().unwrap());
        request
            .headers_mut()
            .insert("OpenAI-Beta", "realtime=v1".parse().unwrap());
        let (ws_stream, _) = match connect_async(request)
            .await
            .map_err(|e| rsipstack::Error::Error(e.to_string()))
        {
            Ok(r) => r,
            Err(e) => {
                info!(
                "Failed to connect to OpenAI Realtime: {:?} {realtime_token:} {realtime_endpoint:?}",
                e
            );
                return Err(e);
            }
        };
        ws_stream.split()
    };

    info!("WebSocket handshake complete");
    let item_session_update = SessionUpdate {
        session: openai_api_rs::realtime::types::Session {
            instructions: Some(prompt.clone()),
            ..Default::default()
        },
        ..Default::default()
    };
    if std::env::var("AZURE_API").is_err() {
        match write.send(item_session_update.into()).await {
            Ok(_) => {}
            Err(e) => {
                info!("send item_create_message failed: {:?}", e);
            }
        }
    } else {
        let item_session_update = serde_json::json!({
            "type": "session.update",
            "session": {
                "instructions": prompt,
                "input_audio_transcription": {
                    "enabled":true,
                    "model": "whisper-1",
                }
            },
        });
        match write
            .send(Message::Text(item_session_update.to_string()))
            .await
        {
            Ok(_) => {}
            Err(e) => {
                info!("send item_create_message failed: {:?}", e);
            }
        }
    }

    let conn_ref = conn.clone();
    let recv_from_rtp_loop = async {
        let buf = &mut [0u8; 2048];

        loop {
            let (n, _) = conn_ref.recv_raw(buf).await.unwrap();
            let samples = match rtp_rs::RtpPacketBuilder::new()
                .payload_type(0)
                .ssrc(ssrc)
                .timestamp(0)
                .payload(&buf[..n])
                .build()
            {
                Ok(r) => r,
                Err(e) => {
                    info!("Failed to build RTP packet: {:?}", e);
                    break;
                }
            };
            let pcm16_samples = pcmu_to_pcm16_u8(&samples);
            let append_audio_message = InputAudioBufferAppend {
                audio: general_purpose::STANDARD_NO_PAD.encode(&pcm16_samples),
                ..Default::default()
            };

            match write.send(append_audio_message.into()).await {
                Ok(_) => {}
                Err(e) => {
                    info!("send append_audio_message failed: {:?}", e);
                    break;
                }
            }
        }
    };
    let output_buf = Arc::new(Mutex::new(vec![0u8; 0]));
    let send_to_rtp = async {
        let mut ticker = tokio::time::interval(tokio::time::Duration::from_millis(20));
        let peer_addr = SipAddr {
            addr: peer_addr.parse().unwrap(),
            r#type: Some(rsip::transport::Transport::Udp),
        };
        let mut seq = 1;
        let mut ts = 0;
        loop {
            let mut chunk = [0u8; SAMPLE_SIZE];
            {
                let mut audio = output_buf.lock().unwrap();
                if audio.len() > SAMPLE_SIZE {
                    chunk.copy_from_slice(&audio[..SAMPLE_SIZE]);
                    audio.copy_within(SAMPLE_SIZE.., 0);
                }
            }

            match RtpPacketBuilder::new()
                .payload_type(0)
                .ssrc(ssrc)
                .sequence(seq.into())
                .timestamp(ts)
                .payload(&chunk)
                .build()
            {
                Ok(r) => match conn.send_raw(&r, &peer_addr).await {
                    Ok(_) => {}
                    Err(e) => {
                        info!("Failed to send RTP: {:?}", e);
                        break;
                    }
                },
                Err(e) => {
                    info!("Failed to build RTP packet: {:?}", e);
                    break;
                }
            };
            ts += chunk.len() as u32;
            seq += 1;
            ticker.tick().await;
        }
    };

    let recv_from_websocket = async {
        while let Some(Ok(message)) = read.next().await {
            match &message {
                Message::Text(_) => {
                    let data = message.into_data();
                    let server_event: ServerEvent = serde_json::from_slice(&data).unwrap();
                    match server_event {
                        ServerEvent::ResponseOutputItemDone(_event) => {
                            info!("received output item done: {_event:?}");
                        }
                        ServerEvent::ResponseTextDone(event) => {
                            info!("received transcript: {event:?}");
                        }
                        ServerEvent::ResponseAudioDelta(event) => {
                            let audio = general_purpose::STANDARD.decode(&event.delta).unwrap();
                            let mut output_buf = output_buf.lock().unwrap();
                            output_buf.extend_from_slice(&audio);
                            info!("received audio: {:?}", audio.len());
                        }
                        ServerEvent::Error(e) => {
                            info!("{e:?}");
                        }
                        _ => {
                            info!("received: {server_event:?}");
                        }
                    }
                }
                Message::Binary(bin) => {
                    info!("received binary: {:?}", bin);
                }
                Message::Close(close) => {
                    info!("received close: {:?}", close);
                    break;
                }
                _ => {}
            }
        }
    };

    select! {
        _ = cancel_token.cancelled() => {
            info!("cancel_token cancelled");
        },
        _ = recv_from_rtp_loop => {
            info!("recv_loop done");
        },
        _ = recv_from_websocket => {
            info!("send_looo done");
        }
        _ = send_to_rtp => {
            info!("send_to_rtp done");
        }
    };
    Ok(())
}
