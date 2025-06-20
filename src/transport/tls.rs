use super::{
    connection::TransportSender,
    sip_addr::SipAddr,
    stream::{StreamConnection, StreamConnectionInner},
    SipConnection, TransportEvent,
};
use crate::{error::Error, Result};
use rsip::SipMessage;
use rustls::client::danger::ServerCertVerifier;
use std::{fmt, net::SocketAddr, sync::Arc};
use tokio::{
    net::{TcpListener, TcpStream},
    select,
};
use tokio_rustls::{
    rustls::{pki_types, ClientConfig, RootCertStore, ServerConfig},
    TlsAcceptor, TlsConnector,
};
use tokio_util::sync::CancellationToken;
use tracing::{debug, error, info, warn};

// TLS configuration
#[derive(Clone, Debug)]
pub struct TlsConfig {
    // Server certificate in PEM format
    pub cert: Option<Vec<u8>>,
    // Server private key in PEM format
    pub key: Option<Vec<u8>>,
    // Client certificate in PEM format
    pub client_cert: Option<Vec<u8>>,
    // Client private key in PEM format
    pub client_key: Option<Vec<u8>>,
    // Root CA certificates in PEM format
    pub ca_certs: Option<Vec<u8>>,
}

impl Default for TlsConfig {
    fn default() -> Self {
        Self {
            cert: None,
            key: None,
            client_cert: None,
            client_key: None,
            ca_certs: None,
        }
    }
}

// TLS Listener Connection Structure
pub struct TlsListenerConnectionInner {
    pub local_addr: SipAddr,
    pub external: Option<SipAddr>,
    pub config: TlsConfig,
}

#[derive(Clone)]
pub struct TlsListenerConnection {
    pub inner: Arc<TlsListenerConnectionInner>,
}

impl TlsListenerConnection {
    pub async fn new(
        local_addr: SipAddr,
        external: Option<SocketAddr>,
        config: TlsConfig,
    ) -> Result<Self> {
        let inner = TlsListenerConnectionInner {
            local_addr,
            external: external.map(|addr| SipAddr {
                r#type: Some(rsip::transport::Transport::Tls),
                addr: addr.into(),
            }),
            config,
        };
        Ok(TlsListenerConnection {
            inner: Arc::new(inner),
        })
    }

    pub async fn serve_listener(
        &self,
        cancel_token: CancellationToken,
        sender: TransportSender,
    ) -> Result<()> {
        let listener = TcpListener::bind(self.inner.local_addr.get_socketaddr()?).await?;
        let acceptor = Self::create_acceptor(&self.inner.config).await?;
        let sender = sender.clone();
        let cancel_token = cancel_token.clone();

        tokio::spawn(async move {
            loop {
                let (stream, remote_addr) = match listener.accept().await {
                    Ok((stream, remote_addr)) => (stream, remote_addr),
                    Err(e) => {
                        warn!("Failed to accept TLS connection: {:?}", e);
                        continue;
                    }
                };

                let acceptor_clone = acceptor.clone();
                let sender_clone = sender.clone();
                let cancel_token = cancel_token.child_token();

                tokio::spawn(async move {
                    // Perform TLS handshake
                    let tls_stream = match acceptor_clone.accept(stream).await {
                        Ok(stream) => stream,
                        Err(e) => {
                            error!("TLS handshake failed: {}", e);
                            return;
                        }
                    };

                    // Create remote SIP address
                    let remote_sip_addr = SipAddr {
                        r#type: Some(rsip::transport::Transport::Tls),
                        addr: remote_addr.into(),
                    };

                    // Create TLS connection
                    let tls_connection = match TlsConnection::from_server_stream(
                        tls_stream,
                        remote_sip_addr.clone(),
                    )
                    .await
                    {
                        Ok(conn) => conn,
                        Err(e) => {
                            error!("Failed to create TLS connection: {:?}", e);
                            return;
                        }
                    };

                    let sip_connection = SipConnection::Tls(tls_connection.clone());
                    let sender_clone = sender_clone.clone();
                    let cancel_token = cancel_token.clone();

                    tokio::spawn(async move {
                        match sender_clone.send(TransportEvent::New(sip_connection.clone())) {
                            Ok(()) => {}
                            Err(e) => {
                                error!(
                                    ?remote_sip_addr,
                                    "Error sending new connection event: {:?}", e
                                );
                                return;
                            }
                        }
                        select! {
                            _ = cancel_token.cancelled() => {}
                            _ = tls_connection.serve_loop(sender_clone.clone()) => {
                                info!(?remote_sip_addr, "TLS connection serve loop completed");
                            }
                        }
                        match sender_clone.send(TransportEvent::Closed(sip_connection)) {
                            Ok(()) => {}
                            Err(e) => {
                                warn!(
                                    ?remote_sip_addr,
                                    "Error sending TLS connection closed event: {:?}", e
                                );
                            }
                        }
                    });
                });
            }
        });
        Ok(())
    }

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

    async fn create_acceptor(config: &TlsConfig) -> Result<TlsAcceptor> {
        // Load certificate chain
        let certs = match &config.cert {
            Some(cert_data) => {
                let mut reader = std::io::BufReader::new(cert_data.as_slice());
                rustls_pemfile::certs(&mut reader)
                    .collect::<std::result::Result<Vec<_>, std::io::Error>>()
                    .map_err(|e| Error::Error(format!("Failed to parse certificate: {}", e)))?
            }
            None => return Err(Error::Error("No certificate provided".to_string())),
        };

        // Load private key
        let key = match &config.key {
            Some(key_data) => {
                let mut reader = std::io::BufReader::new(key_data.as_slice());
                // Try PKCS8 format first
                let keys = rustls_pemfile::pkcs8_private_keys(&mut reader)
                    .collect::<std::result::Result<Vec<_>, std::io::Error>>()
                    .map_err(|e| Error::Error(format!("Failed to parse PKCS8 key: {}", e)))?;

                if !keys.is_empty() {
                    let key_der = pki_types::PrivatePkcs8KeyDer::from(keys[0].clone_key());
                    pki_types::PrivateKeyDer::Pkcs8(key_der)
                } else {
                    // Try PKCS1 format
                    let mut reader = std::io::BufReader::new(key_data.as_slice());
                    let keys = rustls_pemfile::rsa_private_keys(&mut reader)
                        .collect::<std::result::Result<Vec<_>, std::io::Error>>()
                        .map_err(|e| Error::Error(format!("Failed to parse RSA key: {}", e)))?;

                    if !keys.is_empty() {
                        let key_der = pki_types::PrivatePkcs1KeyDer::from(keys[0].clone_key());
                        pki_types::PrivateKeyDer::Pkcs1(key_der)
                    } else {
                        return Err(Error::Error("No valid private key found".to_string()));
                    }
                }
            }
            None => return Err(Error::Error("No private key provided".to_string())),
        };

        // Create server configuration
        let server_config = ServerConfig::builder()
            .with_no_client_auth()
            .with_single_cert(certs, key)
            .map_err(|e| Error::Error(format!("TLS configuration error: {}", e)))?;

        // Create TLS acceptor
        let acceptor = TlsAcceptor::from(Arc::new(server_config));

        Ok(acceptor)
    }
}

impl fmt::Display for TlsListenerConnection {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "TLS Listener {}", self.get_addr())
    }
}

impl fmt::Debug for TlsListenerConnection {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        fmt::Display::fmt(self, f)
    }
}

// Define a type alias for the TLS stream to make the code more readable
type TlsClientStream = tokio_rustls::client::TlsStream<TcpStream>;
type TlsServerStream = tokio_rustls::server::TlsStream<TcpStream>;

// TLS connection - uses enum to handle both client and server streams
#[derive(Clone)]
pub struct TlsConnection {
    inner: TlsConnectionInner,
}

#[derive(Clone)]
enum TlsConnectionInner {
    Client(
        Arc<
            StreamConnectionInner<
                tokio::io::ReadHalf<TlsClientStream>,
                tokio::io::WriteHalf<TlsClientStream>,
            >,
        >,
    ),
    Server(
        Arc<
            StreamConnectionInner<
                tokio::io::ReadHalf<TlsServerStream>,
                tokio::io::WriteHalf<TlsServerStream>,
            >,
        >,
    ),
}

impl TlsConnection {
    // Connect to a remote TLS server
    pub async fn connect(
        remote_addr: &SipAddr,
        custom_verifier: Option<Arc<dyn ServerCertVerifier>>,
    ) -> Result<Self> {
        let root_store = RootCertStore::empty();

        let mut config = ClientConfig::builder()
            .with_root_certificates(root_store)
            .with_no_client_auth();

        match custom_verifier {
            Some(verifier) => {
                config.dangerous().set_certificate_verifier(verifier);
            }
            None => {}
        }
        let connector = TlsConnector::from(Arc::new(config));

        let socket_addr = match &remote_addr.addr.host {
            rsip::host_with_port::Host::Domain(domain) => {
                let port = remote_addr.addr.port.as_ref().map_or(5061, |p| *p.value());
                format!("{}:{}", domain.to_string(), port).parse()?
            }
            rsip::host_with_port::Host::IpAddr(ip) => {
                let port = remote_addr.addr.port.as_ref().map_or(5061, |p| *p.value());
                SocketAddr::new(*ip, port)
            }
        };

        let domain_string = match &remote_addr.addr.host {
            rsip::host_with_port::Host::Domain(domain) => domain.to_string(),
            rsip::host_with_port::Host::IpAddr(ip) => ip.to_string(),
        };

        let server_name = pki_types::ServerName::try_from(domain_string.as_str())
            .map_err(|_| Error::Error(format!("Invalid DNS name: {}", domain_string)))?
            .to_owned();

        let stream = TcpStream::connect(socket_addr).await?;
        let local_addr = SipAddr {
            r#type: Some(rsip::transport::Transport::Tls),
            addr: stream.local_addr()?.into(),
        };

        let tls_stream = connector.connect(server_name, stream).await?;
        let (read_half, write_half) = tokio::io::split(tls_stream);

        let connection = Self {
            inner: TlsConnectionInner::Client(Arc::new(StreamConnectionInner::new(
                local_addr,
                remote_addr.clone(),
                read_half,
                write_half,
            ))),
        };

        info!(
            "Created TLS client connection: {} -> {}",
            connection.get_addr(),
            remote_addr
        );

        Ok(connection)
    }

    // Create TLS connection from existing client TLS stream
    pub async fn from_client_stream(stream: TlsClientStream, remote_addr: SipAddr) -> Result<Self> {
        let local_addr = SipAddr {
            r#type: Some(rsip::transport::Transport::Tls),
            addr: stream.get_ref().0.local_addr()?.into(),
        };

        // Split stream into read and write halves
        let (read_half, write_half) = tokio::io::split(stream);

        // Create TLS connection
        let connection = Self {
            inner: TlsConnectionInner::Client(Arc::new(StreamConnectionInner::new(
                local_addr,
                remote_addr.clone(),
                read_half,
                write_half,
            ))),
        };

        info!(
            "Created TLS client connection: {} <- {}",
            connection.get_addr(),
            remote_addr
        );

        Ok(connection)
    }

    // Create TLS connection from existing server TLS stream
    pub async fn from_server_stream(stream: TlsServerStream, remote_addr: SipAddr) -> Result<Self> {
        let local_addr = SipAddr {
            r#type: Some(rsip::transport::Transport::Tls),
            addr: stream.get_ref().0.local_addr()?.into(),
        };

        // Split stream into read and write halves
        let (read_half, write_half) = tokio::io::split(stream);

        // Create TLS connection
        let connection = Self {
            inner: TlsConnectionInner::Server(Arc::new(StreamConnectionInner::new(
                local_addr,
                remote_addr.clone(),
                read_half,
                write_half,
            ))),
        };

        info!(
            "Created TLS server connection: {} <- {}",
            connection.get_addr(),
            remote_addr
        );

        Ok(connection)
    }

    // Deprecated: use from_client_stream instead
    #[deprecated(note = "Use from_client_stream instead")]
    pub async fn from_stream(stream: TlsClientStream, remote_addr: SipAddr) -> Result<Self> {
        Self::from_client_stream(stream, remote_addr).await
    }

    // Deprecated: use TlsListenerConnection::create_acceptor instead
    #[deprecated(note = "Use TlsListenerConnection::create_acceptor instead")]
    pub async fn create_acceptor(config: &TlsConfig) -> Result<TlsAcceptor> {
        TlsListenerConnection::create_acceptor(config).await
    }

    // Deprecated: use TlsListenerConnection::serve_listener instead
    #[deprecated(note = "Use TlsListenerConnection::serve_listener instead")]
    pub async fn serve_listener(
        listener: TcpListener,
        config: &TlsConfig,
        sender: TransportSender,
        transport_layer: crate::transport::transport_layer::TransportLayerInnerRef,
    ) -> Result<()> {
        // Keep old functionality for backward compatibility but mark as deprecated
        let acceptor = TlsListenerConnection::create_acceptor(config).await?;

        info!("Starting TLS listener");

        loop {
            match listener.accept().await {
                Ok((stream, peer_addr)) => {
                    debug!("New TLS connection from {}", peer_addr);

                    let acceptor_clone = acceptor.clone();
                    let sender_clone = sender.clone();
                    let transport_layer_clone = transport_layer.clone();
                    let cancel_token = transport_layer.cancel_token.child_token();

                    tokio::spawn(async move {
                        // Perform TLS handshake
                        let tls_stream = match acceptor_clone.accept(stream).await {
                            Ok(stream) => stream,
                            Err(e) => {
                                error!("TLS handshake failed: {}", e);
                                return;
                            }
                        };

                        // Create remote SIP address
                        let remote_sip_addr = SipAddr {
                            r#type: Some(rsip::transport::Transport::Tls),
                            addr: peer_addr.into(),
                        };

                        // Create TLS connection
                        let connection = match TlsConnection::from_server_stream(
                            tls_stream,
                            remote_sip_addr.clone(),
                        )
                        .await
                        {
                            Ok(conn) => conn,
                            Err(e) => {
                                error!("Failed to create TLS connection: {}", e);
                                return;
                            }
                        };

                        // Convert to SIP connection
                        let sip_connection = SipConnection::Tls(connection.clone());
                        let connection_addr = connection.get_addr().clone();

                        // Add connection to transport layer if provided
                        transport_layer_clone.add_connection(sip_connection.clone());
                        info!(
                            "Added TLS connection to transport layer: {}",
                            connection_addr
                        );

                        // Notify about new connection
                        if let Err(e) =
                            sender_clone.send(TransportEvent::New(sip_connection.clone()))
                        {
                            error!("Failed to send new connection event: {}", e);
                            return;
                        }
                        select! {
                            _ = cancel_token.cancelled() => {
                            }
                            _ = connection.serve_loop(sender_clone.clone()) => {
                                info!("TLS connection serve loop completed: {}", connection_addr);
                            }
                        }
                        // Remove connection from transport layer when done
                        transport_layer_clone.del_connection(&connection_addr);
                        info!(
                            "Removed TLS connection from transport layer: {}",
                            connection_addr
                        );

                        // Send connection closed event
                        if let Err(e) = sender_clone.send(TransportEvent::Closed(sip_connection)) {
                            warn!("Error sending TLS connection closed event: {:?}", e);
                        }
                    });
                }
                Err(e) => {
                    error!("Error accepting TLS connection: {}", e);
                }
            }
        }
    }
}

// Implement StreamConnection trait for TlsConnection
#[async_trait::async_trait]
impl StreamConnection for TlsConnection {
    fn get_addr(&self) -> &SipAddr {
        match &self.inner {
            TlsConnectionInner::Client(inner) => &inner.local_addr,
            TlsConnectionInner::Server(inner) => &inner.local_addr,
        }
    }

    async fn send_message(&self, msg: SipMessage) -> Result<()> {
        match &self.inner {
            TlsConnectionInner::Client(inner) => inner.send_message(msg).await,
            TlsConnectionInner::Server(inner) => inner.send_message(msg).await,
        }
    }

    async fn send_raw(&self, data: &[u8]) -> Result<()> {
        match &self.inner {
            TlsConnectionInner::Client(inner) => inner.send_raw(data).await,
            TlsConnectionInner::Server(inner) => inner.send_raw(data).await,
        }
    }

    async fn serve_loop(&self, sender: TransportSender) -> Result<()> {
        let sip_connection = SipConnection::Tls(self.clone());
        match &self.inner {
            TlsConnectionInner::Client(inner) => inner.serve_loop(sender, sip_connection).await,
            TlsConnectionInner::Server(inner) => inner.serve_loop(sender, sip_connection).await,
        }
    }

    async fn close(&self) -> Result<()> {
        match &self.inner {
            TlsConnectionInner::Client(inner) => inner.close().await,
            TlsConnectionInner::Server(inner) => inner.close().await,
        }
    }
}

impl fmt::Display for TlsConnection {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match &self.inner {
            TlsConnectionInner::Client(inner) => {
                write!(f, "TLS {} -> {}", inner.local_addr, inner.remote_addr)
            }
            TlsConnectionInner::Server(inner) => {
                write!(f, "TLS {} -> {}", inner.local_addr, inner.remote_addr)
            }
        }
    }
}

impl fmt::Debug for TlsConnection {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        fmt::Display::fmt(self, f)
    }
}
