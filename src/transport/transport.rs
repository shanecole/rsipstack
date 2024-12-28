use std::fmt;

use super::udp::UdpTransport;
use crate::Result;
use rsip::SipMessage;
use tokio::sync::mpsc::{UnboundedReceiver, UnboundedSender};

pub enum TransportEvent {
    IncomingMessage(SipMessage, Transport),
    NewTransport(Transport),
    TransportClosed(Transport),
    Terminate, // Terminate the transport layer
}

pub type TransportReceiver = UnboundedReceiver<TransportEvent>;
pub type TransportSender = UnboundedSender<TransportEvent>;

#[derive(Clone)]
pub enum Transport {
    //Tcp(tcp::TcpTransport),
    //Tls(tls::TlsTransport),
    Udp(UdpTransport),
    //Ws(ws::WsTransport),
}

impl Transport {
    pub fn is_secure(&self) -> bool {
        match self {
            //Transport::Tcp(_) => false,
            //Transport::Tls(_) => true,
            Transport::Udp(_) => false,
            //Transport::Ws(_) => false,
        }
    }

    pub fn is_reliable(&self) -> bool {
        match self {
            Transport::Udp(_) => false,
            _ => true,
        }
    }

    pub fn is_stream(&self) -> bool {
        match self {
            Transport::Udp(_) => false,
            _ => true,
        }
    }

    pub async fn send(&self, msg: rsip::SipMessage) -> Result<()> {
        match self {
            //Transport::Tcp(transport) => transport.send(msg).await,
            //Transport::Tls(transport) => transport.send(msg).await,
            Transport::Udp(transport) => transport.send(msg).await,
            //Transport::Ws(transport) => transport.send(msg).await,
        }
    }
}

impl fmt::Display for Transport {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            //Transport::Tcp(_) => write!(f, "TCP"),
            //Transport::Tls(_) => write!(f, "TLS"),
            Transport::Udp(t) => write!(f, "UDP {}", t),
            //Transport::Ws(_) => write!(f, "WS"),
        }
    }
}
