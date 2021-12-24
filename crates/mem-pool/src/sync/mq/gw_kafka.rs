use std::time::Duration;

use anyhow::{anyhow, Result};
use gw_types::bytes::Bytes;
use gw_types::packed::{RefreshMemBlockMessage, RefreshMemBlockMessageUnion};
use gw_types::prelude::{Builder, Entity, Reader};
use kafka::client::{Compression, FetchOffset, GroupOffsetStorage};
use kafka::producer::{AsBytes, Record};

use crate::sync::subscribe::SubscribeMemPoolService;

use super::{Consume, Produce};

pub(crate) struct Producer {
    producer: kafka::producer::Producer,
    topic: String,
}

impl Producer {
    pub(crate) fn connect(hosts: Vec<String>, topic: String) -> Result<Self> {
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
    subscribe: SubscribeMemPoolService,
}

impl Consumer {
    pub(crate) fn new(
        hosts: Vec<String>,
        topic: String,
        group: String,
        fan_in: SubscribeMemPoolService,
    ) -> Result<Self> {
        let consumer = kafka::consumer::Consumer::from_hosts(hosts)
            .with_topic(topic)
            .with_group(group)
            .with_fallback_offset(FetchOffset::Earliest)
            .with_offset_storage(GroupOffsetStorage::Kafka)
            .create()
            .map_err(|err| anyhow!("Create consumer with error: {:?}", err))?;
        Ok(Self {
            consumer,
            subscribe: fan_in,
        })
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
        let mut current_offset = None;
        // if the current tip is not vaild, we need to exit and retry later.
        let mut early_exit = None;
        for set in msg_sets.iter() {
            'inner: for msg in set.messages().iter() {
                log::trace!(
                    "Recv kafka msg: {}:{}@{}: {:?}",
                    set.topic(),
                    set.partition(),
                    msg.offset,
                    msg.value
                );
                let refresh_msg = RefreshMemBlockMessage::from_slice(msg.value)?;
                let reader = refresh_msg.as_reader();
                let refresh_msg = reader.to_enum();
                match &refresh_msg {
                    gw_types::packed::RefreshMemBlockMessageUnionReader::L2Transaction(tx) => {
                        if let Err(err) = self.subscribe.add_tx(tx.to_entity()) {
                            log::error!("[Subscribe tx] error: {:?}", err);
                        }
                    }
                    gw_types::packed::RefreshMemBlockMessageUnionReader::NextMemBlock(next) => {
                        match self.subscribe.next_mem_block(next.to_entity()) {
                            Ok(None) => {
                                log::warn!("Invalid tip. Need wait for syncing to the new tip.");
                                early_exit = Some(());
                                break 'inner;
                            }
                            Ok(Some(block_number)) => {
                                log::info!("Refresh mem pool to {}", block_number);
                            }
                            Err(err) => {
                                log::error!("[Refresh mem pool] error: {:?}", err);
                            }
                        };
                    }
                };
                current_offset = Some(msg.offset);
            }
            if let Some(offset) = current_offset {
                self.consumer
                    .consume_message(set.topic(), set.partition(), offset)
                    .map_err(|err| anyhow!("Mark consumed message with error: {:?}", err))?;
                self.consumer
                    .commit_consumed()
                    .map_err(|err| anyhow!("Kafka commit consumed failed: {:?}", err))?;
            }
            if early_exit.is_some() {
                break;
            }
        }
        Ok(())
    }
}

struct RefreshMemBlockMessageFacade(Bytes);
impl AsBytes for RefreshMemBlockMessageFacade {
    fn as_bytes(&self) -> &[u8] {
        &self.0
    }
}
