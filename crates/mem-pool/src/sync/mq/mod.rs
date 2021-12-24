use anyhow::Result;
pub(crate) mod gw_kafka;

pub trait Produce {
    type Msg;
    fn send(&mut self, message: Self::Msg) -> Result<()>;
}
pub trait Consume {
    fn poll(&mut self) -> Result<()>;
}
