use crate::transport::stream::StreamConnection;
use crate::transport::tcp::TcpConnection;
use crate::transport::{connection::TransportSender, SipAddr};
use crate::transport::{SipConnection, TransportEvent};
use crate::Result;
use std::fmt;
use std::{net::SocketAddr, sync::Arc};
use tokio::net::TcpListener;
use tokio::select;
use tokio_util::sync::CancellationToken;
use tracing::{error, info, warn};
pub struct TcpListenerConnectionInner {
    pub local_addr: SipAddr,
    pub external: Option<SipAddr>,
}

#[derive(Clone)]
pub struct TcpListenerConnection {
    pub inner: Arc<TcpListenerConnectionInner>,
}

impl TcpListenerConnection {
    pub async fn new(local_addr: SipAddr, external: Option<SocketAddr>) -> Result<Self> {
        let inner = TcpListenerConnectionInner {
            local_addr,
            external: external.map(|addr| SipAddr {
                r#type: Some(rsip::transport::Transport::Tcp),
                addr: addr.into(),
            }),
        };
        Ok(TcpListenerConnection {
            inner: Arc::new(inner),
        })
    }

    pub async fn serve_listener(
        &self,
        cancel_token: CancellationToken,
        sender: TransportSender,
    ) -> Result<()> {
        let listener = TcpListener::bind(self.inner.local_addr.get_socketaddr()?).await?;
        let sender = sender.clone();
        let cancel_token = cancel_token.clone();

        tokio::spawn(async move {
            loop {
                let (stream, remote_addr) = match listener.accept().await {
                    Ok((stream, remote_addr)) => (stream, remote_addr),
                    Err(e) => {
                        warn!("Failed to accept connection: {:?}", e);
                        continue;
                    }
                };
                let local_addr = SipAddr {
                    r#type: Some(rsip::transport::Transport::Tcp),
                    addr: remote_addr.into(),
                };
                let tcp_connection = match TcpConnection::from_stream(stream, local_addr.clone()) {
                    Ok(tcp_connection) => tcp_connection,
                    Err(e) => {
                        error!("Failed to create TCP connection: {:?}", e);
                        continue;
                    }
                };

                let sip_connection = SipConnection::Tcp(tcp_connection.clone());
                let sender_clone = sender.clone();
                let cancel_token = cancel_token.child_token();
                tokio::spawn(async move {
                    match sender_clone.send(TransportEvent::New(sip_connection.clone())) {
                        Ok(()) => {}
                        Err(e) => {
                            error!(?local_addr, "Error sending new connection event: {:?}", e);
                            return;
                        }
                    }
                    select! {
                        _ = cancel_token.cancelled() => {}
                        _ = tcp_connection.serve_loop(sender_clone.clone()) => {
                            info!(?local_addr, "TCP connection serve loop completed");
                        }
                    }
                    match sender_clone.send(TransportEvent::Closed(sip_connection)) {
                        Ok(()) => {}
                        Err(e) => {
                            warn!(
                                ?local_addr,
                                "Error sending connection closed event: {:?}", e
                            );
                        }
                    }
                });
            }
        });
        Ok(())
    }
}

impl TcpListenerConnection {
    pub fn get_addr(&self) -> &SipAddr {
        if let Some(external) = &self.inner.external {
            external
        } else {
            &self.inner.local_addr
        }
    }

    pub async fn close(&self) -> Result<()> {
        Ok(())
    }
}

impl fmt::Display for TcpListenerConnection {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "TCP Listener {}", self.get_addr())
    }
}

impl fmt::Debug for TcpListenerConnection {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        fmt::Display::fmt(self, f)
    }
}
