use super::{channel::ChannelConnection, udp::UdpConnection, ws_wasm::WsWasmConnection};
use crate::Result;
use rsip::{prelude::HeadersExt, HostWithPort, Param, SipMessage};
use std::{fmt, net::SocketAddr};
use tokio::sync::mpsc::{UnboundedReceiver, UnboundedSender};

#[derive(Clone)]
pub enum TransportEvent {
    Incoming(SipMessage, SipConnection, SipAddr),
    New(SipConnection),
    Closed(SipConnection),
    Terminate, // Terminate the transport layer
}
#[derive(Debug, Eq, PartialEq, Clone, Hash)]
pub struct SipAddr {
    pub r#type: Option<rsip::transport::Transport>,
    pub addr: SocketAddr,
}

pub type TransportReceiver = UnboundedReceiver<TransportEvent>;
pub type TransportSender = UnboundedSender<TransportEvent>;

pub const KEEPALIVE_REQUEST: &[u8] = b"\r\n\r\n";
pub const KEEPALIVE_RESPONSE: &[u8] = b"\r\n";

#[derive(Clone, Debug)]
pub enum SipConnection {
    Udp(UdpConnection),
    WsWasm(WsWasmConnection),
    Channel(ChannelConnection),
}

impl SipConnection {
    pub fn is_reliable(&self) -> bool {
        match self {
            SipConnection::Udp(_) => false,
            _ => true,
        }
    }
    pub fn get_addr(&self) -> &SipAddr {
        match self {
            //Transport::Tcp(transport) => transport.get_addr(),
            //Transport::Tls(transport) => transport.get_addr(),
            SipConnection::Udp(transport) => transport.get_addr(),
            SipConnection::WsWasm(transport) => transport.get_addr(),
            SipConnection::Channel(transport) => transport.get_addr(),
            //Transport::Ws(transport) => transport.get_addr(),
        }
    }
    pub async fn send(&self, msg: rsip::SipMessage) -> Result<()> {
        match self {
            //Transport::Tcp(transport) => transport.send(msg).await,
            //Transport::Tls(transport) => transport.send(msg).await,
            SipConnection::Udp(transport) => transport.send(msg).await,
            SipConnection::WsWasm(transport) => transport.send(msg).await,
            SipConnection::Channel(transport) => transport.send(msg).await,
            //Transport::Ws(transport) => transport.send(msg).await,
        }
    }
    pub async fn serve_loop(&self, sender: TransportSender) -> Result<()> {
        match self {
            //Transport::Tcp(transport) => transport.server_loop(sender).await,
            //Transport::Tls(transport) => transport.server_loop(sender).await,
            SipConnection::Udp(transport) => transport.serve_loop(sender).await,
            SipConnection::WsWasm(transport) => transport.serve_loop(sender).await,
            SipConnection::Channel(transport) => transport.serve_loop(sender).await,
            //Transport::Ws(transport) => transport.server_loop(sender).await,
        }
    }
}

impl SipConnection {
    fn parse_target_from_via(via: &rsip::headers::untyped::Via) -> Result<HostWithPort> {
        let mut host_with_port = via.uri()?.host_with_port;
        if let Ok(params) = via.params() {
            for param in params {
                if let Param::Received(v) = param {
                    if let Ok(addr) = v.parse() {
                        host_with_port.host = addr.into();
                        break;
                    }
                }
            }
        }
        Ok(host_with_port)
    }

    pub fn get_target(msg: &rsip::SipMessage) -> Result<SocketAddr> {
        let host_with_port = match msg {
            rsip::SipMessage::Request(req) => req.uri().host_with_port.clone(),
            rsip::SipMessage::Response(res) => Self::parse_target_from_via(res.via_header()?)?,
        };
        host_with_port.try_into().map_err(Into::into)
    }
}

impl fmt::Display for SipConnection {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            //Transport::Tcp(_) => write!(f, "TCP"),
            //Transport::Tls(_) => write!(f, "TLS"),
            SipConnection::Udp(t) => write!(f, "UDP {}", t),
            SipConnection::WsWasm(t) => write!(f, "WS-WASM {}", t),
            SipConnection::Channel(t) => write!(f, "CHANNEL {}", t),
            //Transport::Ws(_) => write!(f, "WS"),
        }
    }
}

impl fmt::Display for SipAddr {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            SipAddr {
                r#type: Some(r#type),
                addr,
            } => write!(f, "{} {}", r#type, addr),
            SipAddr { r#type: None, addr } => write!(f, "{}", addr),
        }
    }
}

impl From<SocketAddr> for SipAddr {
    fn from(addr: SocketAddr) -> Self {
        SipAddr { r#type: None, addr }
    }
}

impl TryFrom<rsip::host_with_port::HostWithPort> for SipAddr {
    type Error = crate::Error;
    fn try_from(host_with_port: rsip::host_with_port::HostWithPort) -> Result<Self> {
        let addr = host_with_port.try_into()?;
        Ok(SipAddr { r#type: None, addr })
    }
}

impl From<UdpConnection> for SipConnection {
    fn from(transport: UdpConnection) -> Self {
        SipConnection::Udp(transport)
    }
}
