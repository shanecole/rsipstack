use super::{channel::ChannelConnection, udp::UdpConnection, ws_wasm::WsWasmConnection};
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
    pub fn update_msg_received(msg: SipMessage, addr: SocketAddr) -> Result<SipMessage> {
        match msg {
            SipMessage::Request(mut req) => {
                let via = req.via_header_mut()?;
                Self::build_via_received(via, addr)?;
                Ok(req.into())
            }
            SipMessage::Response(_) => {
                Ok(msg)
            }
        }
    }

    pub fn build_via_received(via: &mut rsip::headers::Via, addr: SocketAddr) -> Result<()> {
        let received = addr.into();
        let typed_via = via.typed()?;
        if typed_via.uri.host_with_port == received {
            return Ok(());
        }
        *via = typed_via
            .with_param(Param::Received(Received::new(received.host.to_string())))
            .with_param(Param::Other(
                OtherParam::new("rport"),
                Some(OtherParamValue::new(addr.port().to_string())),
            ))
            .into();
        Ok(())
    }

    fn parse_target_from_via(via: &rsip::headers::untyped::Via) -> Result<HostWithPort> {
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

    pub fn get_target_socketaddr(msg: &rsip::SipMessage) -> Result<SocketAddr> {
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
    fn from(connection: UdpConnection) -> Self {
        SipConnection::Udp(connection)
    }
}

impl From<ChannelConnection> for SipConnection {
    fn from(connection: ChannelConnection) -> Self {
        SipConnection::Channel(connection)
    }
}

#[cfg(test)]
mod tests {
    use super::SipConnection;
    use rsip::{headers::*, prelude::HeadersExt, HostWithPort, SipMessage};

    #[test]
    fn test_via_received() {
        let register_req = rsip::message::Request {
            method: rsip::method::Method::Register,
            uri: rsip::Uri {
                scheme: Some(rsip::Scheme::Sip),
                host_with_port: rsip::HostWithPort::try_from("127.0.0.1:2025")
                    .expect("host_port parse")
                    .into(),
                ..Default::default()
            },
            headers: vec![Via::new("SIP/2.0/TLS restsend.com:5061;branch=z9hG4bKnashd92").into()]
                .into(),
            version: rsip::Version::V2,
            body: Default::default(),
        };

        let parse_addr =
            SipConnection::parse_target_from_via(&register_req.via_header().expect("via_header"))
                .expect("get_target_socketaddr");

        let addr = HostWithPort {
            host: "restsend.com".parse().unwrap(),
            port: Some(5061.into()),
        };
        assert_eq!(parse_addr, addr);

        let addr = "127.0.0.1:1234".parse().unwrap();
        let msg = SipConnection::update_msg_received(register_req.into(), addr)
            .expect("update_msg_received");

        match msg {
            SipMessage::Request(req) => {
                let parse_addr =
                    SipConnection::parse_target_from_via(&req.via_header().expect("via_header"))
                        .expect("get_target_socketaddr");
                assert_eq!(parse_addr, addr.into());
            }
            _ => {}
        }
    }
}
