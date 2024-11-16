pub mod udp;
pub mod tcp;
pub mod tls;
pub mod ws;

pub enum Error{

}
#[derive(Debug, PartialEq, Eq)]
pub struct TransportLayer {
    
}

impl TransportLayer {
    pub fn new() -> Self {
        TransportLayer {
        }
    }    
}