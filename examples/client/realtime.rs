use std::sync::{Arc, Mutex};

use base64::engine::general_purpose;
use base64::Engine;
use futures::{SinkExt, StreamExt};
use openai_api_rs::realtime::api::RealtimeClient;
use openai_api_rs::realtime::client_event::{
    ConversationItemCreate, InputAudioBufferAppend, SessionUpdate,
};
use openai_api_rs::realtime::server_event::ServerEvent;
use openai_api_rs::realtime::types::{Item, ItemContent, ItemType};
use rsipstack::transport::connection::SipAddr;
/// SIP <--> Openai realtime example
///
use rsipstack::transport::udp::UdpConnection;
use rsipstack::Result;
use rtp_rs::RtpPacketBuilder;
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

    let model = "gpt-4o-realtime-preview-2024-10-01".to_string();
    let realtime_client = RealtimeClient::new(realtime_token.clone(), model);

    let (mut write, mut read) = match realtime_client
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
    };

    // The Microsoft Azure OpenAI API is not available in the openai_api_rs crate.
    // let mut request = realtime_endpoint
    //     .clone()
    //     .into_client_request()
    //     .map_err(|e| rsipstack::Error::Error(e.to_string()))?;
    // request.headers_mut().insert(
    //     "Authorization",
    //     format!("Bearer {realtime_token}").parse().unwrap(),
    // );
    // request
    //     .headers_mut()
    //     .insert("OpenAI-Beta", "realtime=v1".parse().unwrap());
    // let (ws_stream, _) = match connect_async(request)
    //     .await
    //     .map_err(|e| rsipstack::Error::Error(e.to_string()))
    // {
    //     Ok(r) => r,
    //     Err(e) => {
    //         info!(
    //             "Failed to connect to OpenAI Realtime: {:?} {realtime_token:} {realtime_endpoint:?}",
    //             e
    //         );
    //         return Err(e);
    //     }
    // };
    // let (mut write, mut read) = ws_stream.split();

    info!("WebSocket handshake complete");

    let item_session_update = SessionUpdate {
        session: openai_api_rs::realtime::types::Session {
            input_audio_format: Some(openai_api_rs::realtime::types::AudioFormat::G711ULAW),
            output_audio_format: Some(openai_api_rs::realtime::types::AudioFormat::G711ULAW),
            ..Default::default()
        },
        ..Default::default()
    };

    match write.send(item_session_update.into()).await {
        Ok(_) => {
            info!("send item_create_message success");
        }
        Err(e) => {
            info!("send item_create_message failed: {:?}", e);
        }
    }

    let item_create_message = ConversationItemCreate {
        item: Item {
            r#type: Some(ItemType::Message),
            role: Some(openai_api_rs::realtime::types::ItemRole::User),
            content: Some(vec![ItemContent {
                r#type: openai_api_rs::realtime::types::ItemContentType::InputText,
                text: Some(prompt),
                audio: None,
                transcript: None,
            }]),
            ..Default::default()
        },
        ..Default::default()
    };

    match write.send(item_create_message.into()).await {
        Ok(_) => {
            info!("send item_create_message success");
        }
        Err(e) => {
            info!("send item_create_message failed: {:?}", e);
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

            let append_audio_message = InputAudioBufferAppend {
                audio: general_purpose::STANDARD.encode(&samples),
                ..Default::default()
            };
            match write.send(append_audio_message.into()).await {
                Ok(_) => {
                    info!("send append_audio_message success");
                }
                Err(e) => {
                    info!("send append_audio_message failed: {:?}", e);
                    break;
                }
            }
        }
    };
    let mut output_buf = Arc::new(Mutex::new(vec![0u8; 8192]));
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
                            eprintln!();
                        }
                        ServerEvent::ResponseTextDone(event) => {
                            info!("received transcript: {event:?}");
                        }
                        ServerEvent::ResponseAudioDelta(event) => {
                            let audio = general_purpose::STANDARD.decode(&event.delta).unwrap();
                            let mut output_buf = output_buf.lock().unwrap();
                            output_buf.extend_from_slice(&audio);
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
