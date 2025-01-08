use super::{DialogInner, DialogState, DialogStateReceiver, DialogStateSender};
use crate::Result;
use tokio::sync::mpsc::unbounded_channel;
use tokio_util::sync::CancellationToken;

pub struct Dialog<T>
where
    T: super::DialogMedia,
{
    inner: DialogInner,
    state_receiver: DialogStateReceiver,
    pub state_sender: DialogStateSender,
    pub media: Option<T>,
}

impl<T> Dialog<T>
where
    T: super::DialogMedia,
{
    pub fn new(inner: DialogInner) -> Self {
        let (state_sender, state_receiver) = unbounded_channel();
        Self {
            inner,
            state_receiver,
            state_sender,
            media: None,
        }
    }

    pub fn id(&self) -> &super::DialogID {
        &self.inner.id
    }

    pub async fn receive(&mut self) -> Option<DialogState> {
        self.state_receiver.recv().await
    }

    pub async fn accept(&mut self, token: Option<CancellationToken>) -> Result<()> {
        todo!()
    }

    pub async fn reject(&mut self, token: Option<CancellationToken>) -> Result<()> {
        todo!()
    }

    pub async fn cancel(&mut self, token: Option<CancellationToken>) -> Result<()> {
        todo!()
    }

    pub async fn hangup(&mut self, token: Option<CancellationToken>) -> Result<()> {
        todo!()
    }

    pub async fn reinvite(
        &mut self,
        token: Option<CancellationToken>,
        media: Option<T>,
    ) -> Result<()> {
        todo!()
    }
    ///
    pub async fn do_request(
        &self,
        token: Option<CancellationToken>,
        request: rsip::Request,
    ) -> Result<()> {
        todo!()
    }
}
