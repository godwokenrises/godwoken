use anyhow::Result;
use async_trait::async_trait;

pub(crate) mod gw_kafka;

pub trait Produce {
    type Msg;
    fn send(&mut self, message: Self::Msg) -> Result<()>;
}

#[async_trait]
pub trait Consume {
    async fn poll(&mut self) -> Result<()>;
}
