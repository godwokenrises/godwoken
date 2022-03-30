use anyhow::Result;
use async_trait::async_trait;
use gw_types::bytes::Bytes;
use rdkafka::message::ToBytes;

pub(crate) mod gw_kafka;
pub(crate) mod tokio_kafka;

#[async_trait]
pub trait Produce {
    type Msg;
    async fn send(&mut self, message: Self::Msg) -> Result<()>;
}

#[async_trait]
pub trait Consume {
    async fn poll(&mut self) -> Result<()>;
}

pub(crate) struct RefreshMemBlockMessageFacade(Bytes);
impl ToBytes for RefreshMemBlockMessageFacade {
    fn to_bytes(&self) -> &[u8] {
        &self.0
    }
}
