use tracing::info;

use super::{transport::TransportSender, Transport};
use crate::Result;
use std::sync::Arc;

#[derive(Default)]
pub struct TransportLayerInner {}

#[derive(Default)]
pub struct TransportLayer {
    inner: Arc<TransportLayerInner>,
}

impl TransportLayer {
    pub async fn lookup(&self, uri: &rsip::uri::Uri) -> Result<Transport> {
        info!("TransportLayer::lookup: {}", uri);
        todo!()
    }

    pub async fn serve(&self, sender: TransportSender) -> Result<()> {
        todo!()
    }
}
