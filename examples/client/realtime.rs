use base64::engine::general_purpose;
use base64::Engine;
use futures::{SinkExt, StreamExt};
use openai_api_rs::realtime::api::RealtimeClient;
use openai_api_rs::realtime::client_event::{InputAudioBufferAppend, SessionUpdate};
use openai_api_rs::realtime::types::AudioFormat;
use rsipstack::transport::udp::UdpConnection;
use rsipstack::transport::SipAddr;
use rsipstack::Result;
use rtp_rs::RtpPacketBuilder;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use tokio::select;
use tokio_tungstenite::connect_async;
use tokio_tungstenite::tungstenite::client::IntoClientRequest;
use tokio_tungstenite::tungstenite::protocol::Message;
use tokio_util::sync::CancellationToken;
use tracing::info;

const SAMPLE_SIZE: usize = 160;

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
    let is_azure = realtime_endpoint.to_ascii_lowercase().contains("azure.com");
    let (mut write, mut read) = if is_azure {
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
    } else {
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
    };

    info!("WebSocket handshake complete");

    if is_azure {
        let item_session_update = serde_json::json!({
            "type": "session.update",
            "session": {
                "instructions": prompt,
                "input_audio_format": "g711_alaw",
                "output_audio_format": "g711_alaw",
                "voice":"shimmer",
                "input_audio_transcription": {
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
    } else {
        let item_session_update = SessionUpdate {
            session: openai_api_rs::realtime::types::Session {
                instructions: Some(prompt.clone()),
                input_audio_format: Some(AudioFormat::G711ULAW),
                output_audio_format: Some(AudioFormat::G711ULAW),
                input_audio_transcription: Some(
                    openai_api_rs::realtime::types::AudioTranscription {
                        enabled: true,
                        model: "whisper-1".to_string(),
                    },
                ),
                ..Default::default()
            },
            ..Default::default()
        };
        match write.send(item_session_update.into()).await {
            Ok(_) => {}
            Err(e) => {
                info!("send item_create_message failed: {:?}", e);
            }
        }
    }

    let ready = AtomicBool::new(false);
    let conn_ref = conn.clone();

    let recv_from_rtp_loop = async {
        let buf = &mut [0u8; 2048];
        loop {
            let (n, _) = conn_ref.recv_raw(buf).await.unwrap();

            let samples = match rtp_rs::RtpReader::new(&buf[..n]) {
                Ok(r) => r.payload(),
                Err(e) => {
                    info!("Failed to build RTP packet: {:?}", e);
                    break;
                }
            };
            if !ready.load(Ordering::Relaxed) {
                continue;
            }
            let append_audio_message = InputAudioBufferAppend {
                audio: general_purpose::STANDARD.encode(&samples),
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
            addr: peer_addr.try_into().unwrap(),
            r#type: Some(rsip::transport::Transport::Udp),
        };
        let mut seq = 1;
        let mut ts = 0;
        loop {
            let mut chunk = [0xD5; SAMPLE_SIZE];
            {
                let mut audio = output_buf.lock().unwrap();
                if audio.len() > SAMPLE_SIZE {
                    chunk.copy_from_slice(&audio[..SAMPLE_SIZE]);
                    audio.drain(..SAMPLE_SIZE);
                } else {
                    chunk.iter_mut().for_each(|x| *x = 0xD5);
                }
            }

            match RtpPacketBuilder::new()
                .payload_type(8)
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
            ts += SAMPLE_SIZE as u32;
            seq += 1;
            ticker.tick().await;
        }
    };

    let recv_from_websocket = async {
        while let Some(Ok(message)) = read.next().await {
            match &message {
                Message::Text(_) => {
                    let data = message.into_data();
                    let server_event: serde_json::Value = serde_json::from_slice(&data).unwrap();
                    // Don't use `ServerEvent`` here, it's not compatible with Azure
                    match server_event["type"].as_str().unwrap() {
                        "session.created" => {}
                        "response.text.done" => {
                            info!("received transcript: {server_event:?}");
                        }
                        "response.audio.delta" => {
                            let delta = server_event["delta"].as_str().unwrap();
                            let samples = general_purpose::STANDARD.decode(delta).unwrap();
                            let mut audio = output_buf.lock().unwrap();
                            audio.extend_from_slice(&samples);
                        }
                        "error" => {
                            info!("{server_event:?}");
                            break;
                        }
                        "server_vad" => {
                            info!("server_vad");
                        }
                        "session.updated" => {
                            ready.store(true, Ordering::Relaxed);
                            info!("session.updated {:?}", server_event);
                        }
                        "conversation.item.input_audio_transcription.completed" => {
                            info!(
                                "peer transcription completed {:?}",
                                server_event.get("transcript")
                            );
                        }
                        "response.audio_transcript.delta" => {}
                        "input_audio_buffer.speech_started" => {
                            let mut audio = output_buf.lock().unwrap();
                            audio.clear();
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
    info!("bridge_realtime done");
    Ok(())
}
