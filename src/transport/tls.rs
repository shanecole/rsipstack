use super::{
    connection::{TransportSender, KEEPALIVE_REQUEST, KEEPALIVE_RESPONSE},
    sip_addr::SipAddr,
    stream::StreamConnection,
    SipConnection, TransportEvent,
};
use crate::{error::Error, Result};
use rsip::SipMessage;
use rustls::client::danger::ServerCertVerifier;
use std::{fmt, net::SocketAddr, sync::Arc};
use tokio::{
    io::{AsyncReadExt, AsyncWriteExt},
    net::{TcpListener, TcpStream},
    sync::Mutex,
};
use tokio_rustls::{
    rustls::{pki_types, ClientConfig, RootCertStore, ServerConfig},
    TlsAcceptor, TlsConnector,
};
use tracing::{error, info};

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

// Define a type alias for the TLS stream to make the code more readable
type TlsClientStream = tokio_rustls::client::TlsStream<TcpStream>;

// TLS connection
#[derive(Debug, Clone)]
pub struct TlsConnection {
    remote_addr: SipAddr,
    read_half: Arc<Mutex<Option<tokio::io::ReadHalf<TlsClientStream>>>>,
    write_half: Arc<Mutex<Option<tokio::io::WriteHalf<TlsClientStream>>>>,
}

impl TlsConnection {
    // Create a new TLS connection with an acceptor
    pub fn new_with_acceptor(_acceptor: TlsAcceptor, addr: SipAddr) -> Self {
        let read_half = Arc::new(Mutex::new(None));
        let write_half = Arc::new(Mutex::new(None));

        Self {
            remote_addr: addr,
            read_half,
            write_half,
        }
    }

    // Connect to a remote TLS server
    pub async fn connect(
        remote_addr: &SipAddr,
        custom_verifier: Option<Arc<dyn ServerCertVerifier>>,
    ) -> Result<Self> {
        // Create TLS configuration
        let root_store = RootCertStore::empty();

        // Use webpki-roots to add trusted root certificates
        // Create client configuration
        let mut config = ClientConfig::builder()
            .with_root_certificates(root_store)
            .with_no_client_auth();

        match custom_verifier {
            Some(verifier) => {
                config.dangerous().set_certificate_verifier(verifier);
            }
            None => {}
        }
        // Create TLS connector
        let connector = TlsConnector::from(Arc::new(config));

        // Connect to remote server
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

        // Perform TLS handshake
        let tls_stream = connector.connect(server_name, stream).await?;

        // Split stream into read and write halves
        let (read_half, write_half) = tokio::io::split(tls_stream);

        // Create TLS connection
        let connection = Self {
            remote_addr: remote_addr.clone(),
            read_half: Arc::new(Mutex::new(Some(read_half))),
            write_half: Arc::new(Mutex::new(Some(write_half))),
        };

        Ok(connection)
    }

    // Create TLS connection from existing TLS stream
    pub async fn from_stream(stream: TlsClientStream, remote_addr: SipAddr) -> Result<Self> {
        // Split stream into read and write halves
        let (read_half, write_half) = tokio::io::split(stream);

        // Create TLS connection
        let connection = Self {
            remote_addr,
            read_half: Arc::new(Mutex::new(Some(read_half))),
            write_half: Arc::new(Mutex::new(Some(write_half))),
        };

        Ok(connection)
    }

    // Create TLS acceptor from configuration
    pub async fn create_acceptor(config: &TlsConfig) -> Result<TlsAcceptor> {
        // Load certificates
        let certs = match &config.cert {
            Some(cert_data) => {
                let mut reader = std::io::BufReader::new(cert_data.as_slice());
                // Use rustls-pemfile to parse certificates
                let certs = rustls_pemfile::certs(&mut reader)
                    .collect::<std::result::Result<Vec<_>, std::io::Error>>()
                    .map_err(|e| Error::Error(format!("Failed to parse certificate: {}", e)))?;
                certs
                    .into_iter()
                    .map(pki_types::CertificateDer::from)
                    .collect()
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

    // Serve TLS listener
    pub async fn serve_listener(
        listener: TcpListener,
        config: &TlsConfig,
        sender: TransportSender,
    ) -> Result<()> {
        // Create TLS acceptor
        let acceptor = Self::create_acceptor(config).await?;

        // Accept connections
        loop {
            // Accept TCP connection
            let (stream, peer_addr) = match listener.accept().await {
                Ok(conn) => conn,
                Err(e) => {
                    error!("Failed to accept connection: {}", e);
                    continue;
                }
            };

            // Clone acceptor and sender for this connection
            let acceptor = acceptor.clone();
            let sender = sender.clone();

            // Handle connection in a separate task
            tokio::spawn(async move {
                // Perform TLS handshake
                let tls_stream = match acceptor.accept(stream).await {
                    Ok(stream) => stream,
                    Err(e) => {
                        error!("TLS handshake failed: {}", e);
                        return;
                    }
                };

                // Create remote SIP address
                let remote_sip_addr = SipAddr {
                    r#type: Some(rsip::Transport::Tls),
                    addr: peer_addr.into(),
                };

                let (server_io, _server_session) = tls_stream.into_inner();

                let local_addr = match server_io.local_addr() {
                    Ok(addr) => addr,
                    Err(e) => {
                        error!("Failed to get local address: {}", e);
                        return;
                    }
                };

                let root_store = RootCertStore::empty();

                // Use webpki-roots to add trusted root certificates
                //root_store.add_server_trust_anchors(&webpki_roots::TLS_SERVER_ROOTS);

                let client_config = ClientConfig::builder()
                    .with_root_certificates(root_store)
                    .with_no_client_auth();

                let domain_string = match &remote_sip_addr.addr.host {
                    rsip::host_with_port::Host::Domain(domain) => domain.to_string(),
                    rsip::host_with_port::Host::IpAddr(ip) => ip.to_string(),
                };

                let server_name = match pki_types::ServerName::try_from(domain_string.as_str()) {
                    Ok(name) => name.to_owned(),
                    Err(_) => {
                        error!("Invalid DNS name: {}", domain_string);
                        return;
                    }
                };

                let client_stream = match TcpStream::connect(local_addr).await {
                    Ok(stream) => stream,
                    Err(e) => {
                        error!("Failed to create TCP connection: {}", e);
                        return;
                    }
                };

                let connector = TlsConnector::from(Arc::new(client_config));
                let client_tls_stream = match connector.connect(server_name, client_stream).await {
                    Ok(stream) => stream,
                    Err(e) => {
                        error!("Failed to create client TLS stream: {}", e);
                        return;
                    }
                };

                // Create TLS connection
                let connection =
                    match TlsConnection::from_stream(client_tls_stream, remote_sip_addr.clone())
                        .await
                    {
                        Ok(conn) => conn,
                        Err(e) => {
                            error!("Failed to create TLS connection: {}", e);
                            return;
                        }
                    };

                // Convert to SIP connection
                let sip_connection = super::connection::SipConnection::from(connection);

                // Notify about new connection
                if let Err(e) = sender.send(super::connection::TransportEvent::New(
                    sip_connection.clone(),
                )) {
                    error!("Failed to send new connection event: {}", e);
                    return;
                }

                // Serve connection
                if let Err(e) = sip_connection.serve_loop(sender).await {
                    error!("Error serving TLS connection: {}", e);
                }
            });
        }
    }
}

// Implement StreamConnection trait for TlsConnection
#[async_trait::async_trait]
impl StreamConnection for TlsConnection {
    fn get_addr(&self) -> &SipAddr {
        &self.remote_addr
    }

    async fn send_message(&self, msg: SipMessage) -> Result<()> {
        info!("TlsConnection send:{}", msg);
        let mut write_half_guard = self.write_half.lock().await;
        if let Some(write_half) = &mut *write_half_guard {
            let mut buf = Vec::new();
            buf.extend_from_slice(msg.to_string().as_bytes());
            write_half.write_all(&buf).await?;

            Ok(())
        } else {
            Err(Error::Error("TLS connection not established".to_string()))
        }
    }

    async fn send_raw(&self, data: &[u8]) -> Result<()> {
        let mut write_half_guard = self.write_half.lock().await;
        if let Some(write_half) = &mut *write_half_guard {
            write_half.write_all(data).await?;

            Ok(())
        } else {
            Err(Error::Error("TLS connection not established".to_string()))
        }
    }

    async fn close(&self) -> Result<()> {
        let mut write_half_guard = self.write_half.lock().await;
        if let Some(write_half) = &mut *write_half_guard {
            write_half.shutdown().await?;
            *write_half_guard = None;
        }
        Ok(())
    }

    async fn serve_loop(&self, sender: TransportSender) -> Result<()> {
        let mut buf = vec![0u8; 2048];
        let sip_connection = SipConnection::Tls(self.clone());
        let remote_addr = self.remote_addr.clone();
        let mut read_half_guard = self.read_half.lock().await;
        loop {
            let mut len = 0;
            if let Some(read_half) = &mut *read_half_guard {
                len = read_half.read(&mut buf).await?;
                if len <= 0 {
                    continue;
                }
            } else {
                continue;
            }

            match &buf[..len] {
                KEEPALIVE_REQUEST => match self.send_raw(KEEPALIVE_RESPONSE).await {
                    Ok(_) => continue,
                    Err(e) => {
                        error!("Failed to send keepalive response: {}", e);
                        break;
                    }
                },
                KEEPALIVE_RESPONSE => continue,
                _ => {
                    if buf.iter().all(|&b| b.is_ascii_whitespace()) {
                        continue;
                    }
                }
            }

            let undecoded = match std::str::from_utf8(&buf[..len]) {
                Ok(s) => s,
                Err(e) => {
                    info!("decoding text ferror: {} buf: {:?}", e, &buf[..len]);
                    continue;
                }
            };

            let sip_msg = match rsip::SipMessage::try_from(undecoded) {
                Ok(msg) => msg,
                Err(e) => {
                    info!("error parsing SIP message error: {} buf: {}", e, undecoded);
                    continue;
                }
            };

            if let Err(e) = sender.send(TransportEvent::Incoming(
                sip_msg,
                sip_connection.clone(),
                remote_addr.clone(),
            )) {
                error!("Error sending incoming message: {:?}", e);
                break;
            }
        }
        info!("TLS connection closed");
        Ok(())
    }
}

impl fmt::Display for TlsConnection {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "TLS {}", self.remote_addr)
    }
}
