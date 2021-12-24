use std::time::Duration;

use anyhow::{anyhow, Result};
use gw_types::bytes::Bytes;
use gw_types::packed::{RefreshMemBlockMessage, RefreshMemBlockMessageUnion};
use gw_types::prelude::{Builder, Entity, Reader};
use kafka::client::{Compression, FetchOffset, GroupOffsetStorage};
use kafka::producer::{AsBytes, Record};

use crate::sync::fan_in::FanInMemBlock;

use super::{Consume, Produce};

pub(crate) struct Producer {
    producer: kafka::producer::Producer,
    topic: String,
}

impl Producer {
    pub(crate) fn new(hosts: Vec<String>, topic: String) -> Result<Self> {
        let mut client = kafka::client::KafkaClient::new(hosts);
        client.load_metadata(&[&topic]).map_err(|err| {
            anyhow!(
                "topic {:?} not found, load metadata error: {:?}",
                &topic,
                err
            )
        })?;
        let producer = kafka::producer::Producer::from_client(client)
            .with_ack_timeout(Duration::from_secs(1))
            .with_required_acks(kafka::client::RequiredAcks::One)
            .with_compression(Compression::SNAPPY)
            .with_connection_idle_timeout(Duration::from_secs(5))
            .create()
            .map_err(|err| anyhow!("Init producer failed: {:?}", err))?;

        Ok(Self { producer, topic })
    }
}

impl Produce for Producer {
    type Msg = RefreshMemBlockMessageUnion;

    fn send(&mut self, message: Self::Msg) -> Result<()> {
        let msg = RefreshMemBlockMessage::new_builder().set(message).build();

        let bytes = msg.as_bytes();
        log::trace!("Producer send msg: {:?}", &bytes.to_vec());
        let message = RefreshMemBlockMessageFacade(bytes);
        let rec = Record::from_value(&self.topic, message);

        if let Err(err) = self.producer.send(&rec) {
            log::warn!("Publish message failed, error: {:?}", err);
        }

        Ok(())
    }
}

pub(crate) struct Consumer {
    consumer: kafka::consumer::Consumer,
    fan_in: FanInMemBlock,
}

impl Consumer {
    pub(crate) fn new(
        hosts: Vec<String>,
        topic: String,
        group: String,
        fan_in: FanInMemBlock,
    ) -> Result<Self> {
        let consumer = kafka::consumer::Consumer::from_hosts(hosts)
            .with_topic(topic)
            .with_group(group)
            .with_fallback_offset(FetchOffset::Earliest)
            .with_offset_storage(GroupOffsetStorage::Kafka)
            .create()
            .map_err(|err| anyhow!("Create consumer with error: {:?}", err))?;
        Ok(Self { consumer, fan_in })
    }
}

impl Consume for Consumer {
    fn poll(&mut self) -> Result<()> {
        let msg_sets = self
            .consumer
            .poll()
            .map_err(|err| anyhow!("Poll message from kafka with error: {:?}", err))?;
        if msg_sets.is_empty() {
            return Ok(());
        }
        for set in msg_sets.iter() {
            for msg in set.messages().iter() {
                log::trace!(
                    "Recv kafka msg: {}:{}@{}: {:?}",
                    set.topic(),
                    set.partition(),
                    msg.offset,
                    msg.value
                );
                let msg = RefreshMemBlockMessage::from_slice(msg.value)?;
                let reader = msg.as_reader();
                let msg = reader.to_enum();
                match &msg {
                    gw_types::packed::RefreshMemBlockMessageUnionReader::L2Transaction(tx) => {
                        if let Err(err) = self.fan_in.add_tx(tx.to_entity()) {
                            log::error!("[Fan in tx to mem block] error: {:?}", err);
                        }
                    }
                    gw_types::packed::RefreshMemBlockMessageUnionReader::NextMemBlock(nxt) => {
                        if let Err(err) = self.fan_in.next_mem_block(nxt.to_entity()) {
                            log::error!("[Fan in next mem block] error: {:?}", err);
                        }
                    }
                };
            }
        }
        self.consumer
            .commit_consumed()
            .map_err(|err| anyhow!("Kafka commit consumed failed: {:?}", err))?;
        Ok(())
    }
}

struct RefreshMemBlockMessageFacade(Bytes);
impl AsBytes for RefreshMemBlockMessageFacade {
    fn as_bytes(&self) -> &[u8] {
        &self.0
    }
}
