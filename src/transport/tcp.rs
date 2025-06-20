use crate::{
    transport::{
        connection::TransportSender,
        sip_addr::SipAddr,
        stream::{StreamConnection, StreamConnectionInner},
        SipConnection,
    },
    Result,
};
use rsip::SipMessage;
use std::{fmt, sync::Arc};
use tokio::net::TcpStream;
use tracing::info;

type TcpInner =
    StreamConnectionInner<tokio::io::ReadHalf<TcpStream>, tokio::io::WriteHalf<TcpStream>>;

#[derive(Clone)]
pub struct TcpConnection {
    pub inner: Arc<TcpInner>,
}

impl TcpConnection {
    pub async fn connect(remote: &SipAddr) -> Result<Self> {
        let socket_addr = remote.get_socketaddr()?;
        let stream = TcpStream::connect(socket_addr).await?;

        let local_addr = SipAddr {
            r#type: Some(rsip::transport::Transport::Tcp),
            addr: stream.local_addr()?.into(),
        };

        let (read_half, write_half) = tokio::io::split(stream);

        let connection = TcpConnection {
            inner: Arc::new(StreamConnectionInner::new(
                local_addr,
                remote.clone(),
                read_half,
                write_half,
            )),
        };

        info!(
            "Created TCP client connection: {} -> {}",
            connection.get_addr(),
            remote
        );

        Ok(connection)
    }

    pub fn from_stream(stream: TcpStream, local_addr: SipAddr) -> Result<Self> {
        let remote_addr = stream.peer_addr()?;
        let remote_sip_addr = SipAddr {
            r#type: Some(rsip::transport::Transport::Tcp),
            addr: remote_addr.into(),
        };

        let (read_half, write_half) = tokio::io::split(stream);

        let connection = TcpConnection {
            inner: Arc::new(StreamConnectionInner::new(
                local_addr,
                remote_sip_addr,
                read_half,
                write_half,
            )),
        };

        info!(
            "Created TCP server connection: {} <- {}",
            connection.get_addr(),
            remote_addr
        );

        Ok(connection)
    }

    // pub async fn create_listener(local: std::net::SocketAddr) -> Result<(TcpListener, SipAddr)> {
    //     let listener = TcpListener::bind(local).await?;
    //     let local_addr = listener.local_addr()?;

    //     let sip_addr = SipAddr {
    //         r#type: Some(rsip::transport::Transport::Tcp),
    //         addr: local_addr.into(),
    //     };

    //     info!("Created TCP listener on {}", local_addr);

    //     Ok((listener, sip_addr))
    // }

    // pub async fn serve_listener(
    //     listener: TcpListener,
    //     local_addr: SipAddr,
    //     sender: TransportSender,
    //     transport_layer: TransportLayerInnerRef,
    // ) -> Result<()> {
    //     info!("Starting TCP listener on {}", local_addr);

    //     loop {
    //         match listener.accept().await {
    //             Ok((stream, remote_addr)) => {
    //                 debug!("New TCP connection from {}", remote_addr);

    //                 let tcp_connection =
    //                     TcpConnection::from_stream(stream, local_addr.clone()).await?;
    //                 let sip_connection = SipConnection::Tcp(tcp_connection.clone());

    //                 // Add connection to transport layer if provided
    //                 transport_layer.add_connection(sip_connection.clone());
    //                 info!(
    //                     "Added TCP connection to transport layer: {}",
    //                     tcp_connection.get_addr()
    //                 );

    //                 let sender_clone = sender.clone();
    //                 let transport_layer_clone = transport_layer.clone();
    //                 let connection_addr = tcp_connection.get_addr().clone();
    //                 let cancel_token = transport_layer.cancel_token.child_token();

    //                 tokio::spawn(async move {
    //                     // Send new connection event
    //                     if let Err(e) =
    //                         sender_clone.send(TransportEvent::New(sip_connection.clone()))
    //                     {
    //                         error!("Error sending new connection event: {:?}", e);
    //                         return;
    //                     }
    //                     select! {
    //                         _ = cancel_token.cancelled() => {}
    //                         _ = tcp_connection.serve_loop(sender_clone.clone()) => {
    //                             info!("TCP connection serve loop completed: {}", connection_addr);
    //                         }
    //                     }
    //                     // Remove connection from transport layer when done
    //                     transport_layer_clone.del_connection(&connection_addr);
    //                     info!(
    //                         "Removed TCP connection from transport layer: {}",
    //                         connection_addr
    //                     );

    //                     // Send connection closed event
    //                     if let Err(e) = sender_clone.send(TransportEvent::Closed(sip_connection)) {
    //                         warn!("Error sending connection closed event: {:?}", e);
    //                     }
    //                 });
    //             }
    //             Err(e) => {
    //                 error!("Error accepting TCP connection: {}", e);
    //             }
    //         }
    //     }
    // }
}

#[async_trait::async_trait]
impl StreamConnection for TcpConnection {
    fn get_addr(&self) -> &SipAddr {
        &self.inner.local_addr
    }

    async fn send_message(&self, msg: SipMessage) -> Result<()> {
        self.inner.send_message(msg).await
    }

    async fn send_raw(&self, data: &[u8]) -> Result<()> {
        self.inner.send_raw(data).await
    }

    async fn serve_loop(&self, sender: TransportSender) -> Result<()> {
        let sip_connection = SipConnection::Tcp(self.clone());
        self.inner.serve_loop(sender, sip_connection).await
    }

    async fn close(&self) -> Result<()> {
        self.inner.close().await
    }
}

impl fmt::Display for TcpConnection {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "TCP {} -> {}",
            self.inner.local_addr, self.inner.remote_addr
        )
    }
}

impl fmt::Debug for TcpConnection {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        fmt::Display::fmt(self, f)
    }
}
