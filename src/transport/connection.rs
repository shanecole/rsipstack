use super::{channel::ChannelConnection, udp::UdpConnection, SipAddr};
use crate::Result;
use rsip::{
    param::{OtherParam, OtherParamValue, Received},
    prelude::{HeadersExt, ToTypedHeader},
    HostWithPort, Param, SipMessage,
};
use std::{fmt, net::SocketAddr};
use tokio::sync::mpsc::{UnboundedReceiver, UnboundedSender};

#[derive(Clone)]
pub enum TransportEvent {
    Incoming(SipMessage, SipConnection, SipAddr),
    New(SipConnection),
    Closed(SipConnection),
}

pub type TransportReceiver = UnboundedReceiver<TransportEvent>;
pub type TransportSender = UnboundedSender<TransportEvent>;

pub const KEEPALIVE_REQUEST: &[u8] = b"\r\n\r\n";
pub const KEEPALIVE_RESPONSE: &[u8] = b"\r\n";

#[derive(Clone, Debug)]
pub enum SipConnection {
    Udp(UdpConnection),
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
            SipConnection::Udp(transport) => transport.get_addr(),
            SipConnection::Channel(transport) => transport.get_addr(),
        }
    }
    pub async fn send(&self, msg: rsip::SipMessage, destination: Option<&SipAddr>) -> Result<()> {
        match self {
            SipConnection::Udp(transport) => transport.send(msg, destination).await,
            SipConnection::Channel(transport) => transport.send(msg).await,
        }
    }
    pub async fn serve_loop(&self, sender: TransportSender) -> Result<()> {
        match self {
            SipConnection::Udp(transport) => transport.serve_loop(sender).await,
            SipConnection::Channel(transport) => transport.serve_loop(sender).await,
        }
    }
}

impl SipConnection {
    pub fn update_msg_received(msg: SipMessage, addr: SocketAddr) -> Result<SipMessage> {
        match msg {
            SipMessage::Request(mut req) => {
                let via = req.via_header_mut()?;
                Self::build_via_received(via, addr)?;
                Ok(req.into())
            }
            SipMessage::Response(_) => Ok(msg),
        }
    }

    pub fn build_via_received(via: &mut rsip::headers::Via, addr: SocketAddr) -> Result<()> {
        let received = addr.into();
        let mut typed_via = via.typed()?;
        if typed_via.uri.host_with_port == received {
            return Ok(());
        }
        typed_via.params.retain(|param| {
            if let Param::Other(key, _) = param {
                !key.value().eq_ignore_ascii_case("rport")
            } else {
                true
            }
        });
        *via = typed_via
            .with_param(Param::Received(Received::new(received.host.to_string())))
            .with_param(Param::Other(
                OtherParam::new("rport"),
                Some(OtherParamValue::new(addr.port().to_string())),
            ))
            .into();
        Ok(())
    }

    pub fn parse_target_from_via(via: &rsip::headers::untyped::Via) -> Result<HostWithPort> {
        let mut host_with_port = via.uri()?.host_with_port;
        if let Ok(params) = via.params().as_ref() {
            for param in params {
                match param {
                    Param::Received(v) => {
                        if let Ok(addr) = v.parse() {
                            host_with_port.host = addr.into();
                        }
                    }
                    Param::Other(key, Some(value)) if key.value().eq_ignore_ascii_case("rport") => {
                        if let Ok(port) = value.value().try_into() {
                            host_with_port.port = Some(port);
                        }
                    }
                    _ => {}
                }
            }
        }
        Ok(host_with_port)
    }

    pub fn get_destination(msg: &rsip::SipMessage) -> Result<SocketAddr> {
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
            SipConnection::Udp(t) => write!(f, "UDP {}", t),
            SipConnection::Channel(t) => write!(f, "CHANNEL {}", t),
        }
    }
}

impl From<UdpConnection> for SipConnection {
    fn from(connection: UdpConnection) -> Self {
        SipConnection::Udp(connection)
    }
}

impl From<ChannelConnection> for SipConnection {
    fn from(connection: ChannelConnection) -> Self {
        SipConnection::Channel(connection)
    }
}

impl Into<HostWithPort> for SipAddr {
    fn into(self) -> HostWithPort {
        self.addr
    }
}
impl Into<rsip::Uri> for SipAddr {
    fn into(self) -> rsip::Uri {
        let scheme = match self.r#type {
            Some(rsip::transport::Transport::Wss) | Some(rsip::transport::Transport::Tls) => {
                rsip::Scheme::Sips
            }
            _ => rsip::Scheme::Sip,
        };
        rsip::Uri {
            scheme: Some(scheme),
            host_with_port: self.addr,
            ..Default::default()
        }
    }
}
