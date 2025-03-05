use crate::{
    transport::{
        connection::{TransportSender, KEEPALIVE_REQUEST, KEEPALIVE_RESPONSE},
        SipAddr, SipConnection, TransportEvent,
    },
    Result,
};
use bytes::{Buf, BytesMut};
use rsip::SipMessage;
use std::sync::Arc;
use tokio::{
    io::{AsyncRead, AsyncReadExt, AsyncWrite, AsyncWriteExt},
    sync::Mutex,
};
use tokio_util::codec::{Decoder, Encoder};
use tracing::{debug, error, warn};

const MAX_SIP_MESSAGE_SIZE: usize = 65535;

pub struct SipCodec {
    max_size: usize,
}

impl SipCodec {
    pub fn new() -> Self {
        Self {
            max_size: MAX_SIP_MESSAGE_SIZE,
        }
    }
}

impl Default for SipCodec {
    fn default() -> Self {
        Self::new()
    }
}

impl Decoder for SipCodec {
    type Item = SipMessage;
    type Error = crate::Error;

    fn decode(&mut self, src: &mut BytesMut) -> Result<Option<Self::Item>> {
        if src.len() >= 4 && &src[0..4] == KEEPALIVE_REQUEST {
            src.advance(4);
            return Err(crate::Error::Keepalive);
        }

        if src.len() >= 2 && &src[0..2] == KEEPALIVE_RESPONSE {
            src.advance(2);
            return Err(crate::Error::Keepalive);
        }

        let data = match std::str::from_utf8(&src[..]) {
            Ok(s) => s,
            Err(_) => {
                if src.len() > self.max_size {
                    return Err(crate::Error::Error("SIP message too large".to_string()));
                }
                return Ok(None);
            }
        };

        if !data.contains("\r\n\r\n") {
            if src.len() > self.max_size {
                return Err(crate::Error::Error("SIP message too large".to_string()));
            }
            return Ok(None);
        }

        match SipMessage::try_from(data) {
            Ok(msg) => {
                let msg_len = data.find("\r\n\r\n").unwrap() + 4;
                src.advance(msg_len);
                Ok(Some(msg))
            }
            Err(e) => {
                if let Some(pos) = data[1..].find("\r\n\r\n") {
                    src.advance(pos + 5);
                } else {
                    src.clear();
                }
                Err(crate::Error::Error(format!(
                    "Failed to parse SIP message: {}",
                    e
                )))
            }
        }
    }
}

impl Encoder<SipMessage> for SipCodec {
    type Error = crate::Error;

    fn encode(&mut self, item: SipMessage, dst: &mut BytesMut) -> Result<()> {
        let data = item.to_string();
        dst.extend_from_slice(data.as_bytes());
        Ok(())
    }
}

#[async_trait::async_trait]
pub trait StreamConnection: Send + Sync + 'static {
    fn get_addr(&self) -> &SipAddr;

    async fn send_message(&self, msg: SipMessage) -> Result<()>;

    async fn send_raw(&self, data: &[u8]) -> Result<()>;

    async fn serve_loop(&self, sender: TransportSender) -> Result<()>;

    async fn close(&self) -> Result<()>;
}

pub async fn handle_stream<S>(
    stream: S,
    local_addr: SipAddr,
    remote_addr: SipAddr,
    connection: SipConnection,
    sender: TransportSender,
) -> Result<()>
where
    S: AsyncRead + AsyncWrite + Unpin + Send + 'static,
{
    let (mut read_half, write_half) = tokio::io::split(stream);
    let write_half = Arc::new(Mutex::new(write_half));

    let mut codec = SipCodec::new();
    let mut buffer = BytesMut::with_capacity(4096);

    sender.send(TransportEvent::New(connection.clone()))?;

    let mut read_buf = [0u8; 4096];

    loop {
        match read_half.read(&mut read_buf).await {
            Ok(0) => {
                debug!("Connection closed: {}", local_addr);
                break;
            }
            Ok(n) => {
                buffer.extend_from_slice(&read_buf[0..n]);

                loop {
                    match codec.decode(&mut buffer) {
                        Ok(Some(msg)) => {
                            debug!("Received message from {}: {:?}", remote_addr, msg);

                            sender.send(TransportEvent::Incoming(
                                msg,
                                connection.clone(),
                                remote_addr.clone(),
                            ))?;
                        }
                        Ok(None) => {
                            break;
                        }
                        Err(crate::Error::Keepalive) => {
                            let mut lock = write_half.lock().await;
                            lock.write_all(KEEPALIVE_RESPONSE).await?;
                            lock.flush().await?;
                        }
                        Err(e) => {
                            warn!("Error decoding message from {}: {:?}", remote_addr, e);
                        }
                    }
                }
            }
            Err(e) => {
                error!("Error reading from stream: {}", e);
                break;
            }
        }
    }

    sender.send(TransportEvent::Closed(connection))?;

    Ok(())
}

pub async fn send_to_stream<W>(write_half: &Arc<Mutex<W>>, msg: SipMessage) -> Result<()>
where
    W: AsyncWrite + Unpin + Send,
{
    let data = msg.to_string();
    let mut lock = write_half.lock().await;
    lock.write_all(data.as_bytes()).await?;
    lock.flush().await?;
    Ok(())
}

pub async fn send_raw_to_stream<W>(write_half: &Arc<Mutex<W>>, data: &[u8]) -> Result<()>
where
    W: AsyncWrite + Unpin + Send,
{
    let mut lock = write_half.lock().await;
    lock.write_all(data).await?;
    lock.flush().await?;
    Ok(())
}
