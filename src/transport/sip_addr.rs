use crate::Result;
use rsip::{host_with_port, HostWithPort};
use std::{fmt, hash::Hash, net::SocketAddr};

#[derive(Debug, Eq, PartialEq, Clone)]
pub struct SipAddr {
    pub r#type: Option<rsip::transport::Transport>,
    pub addr: HostWithPort,
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

impl Hash for SipAddr {
    fn hash<H: std::hash::Hasher>(&self, state: &mut H) {
        self.r#type.hash(state);
        match self.addr.host {
            host_with_port::Host::Domain(ref domain) => domain.hash(state),
            host_with_port::Host::IpAddr(ref ip_addr) => ip_addr.hash(state),
        }
        self.addr.port.map(|port| port.value().hash(state));
    }
}

impl SipAddr {
    pub fn new(transport: rsip::transport::Transport, addr: HostWithPort) -> Self {
        SipAddr {
            r#type: Some(transport),
            addr,
        }
    }

    pub fn get_socketaddr(&self) -> Result<SocketAddr> {
        match &self.addr.host {
            host_with_port::Host::Domain(domain) => Err(crate::Error::Error(format!(
                "Cannot convert domain {} to SocketAddr",
                domain
            ))),
            host_with_port::Host::IpAddr(ip_addr) => {
                let port = self.addr.port.map_or(5060, |p| p.value().to_owned());
                Ok(SocketAddr::new(ip_addr.to_owned(), port))
            }
        }
    }
}
impl From<SocketAddr> for SipAddr {
    fn from(addr: SocketAddr) -> Self {
        let host_with_port = HostWithPort {
            host: addr.ip().into(),
            port: Some(addr.port().into()),
        };
        SipAddr {
            r#type: None,
            addr: host_with_port,
        }
    }
}

impl From<rsip::host_with_port::HostWithPort> for SipAddr {
    fn from(host_with_port: rsip::host_with_port::HostWithPort) -> Self {
        SipAddr {
            r#type: None,
            addr: host_with_port,
        }
    }
}

impl TryFrom<&rsip::Uri> for SipAddr {
    type Error = crate::Error;

    fn try_from(uri: &rsip::Uri) -> Result<Self> {
        let transport = uri.transport().cloned();
        Ok(SipAddr {
            r#type: transport,
            addr: uri.host_with_port.clone(),
        })
    }
}
