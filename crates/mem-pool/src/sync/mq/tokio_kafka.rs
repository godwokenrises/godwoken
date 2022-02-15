use futures::StreamExt;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use anyhow::Result;
use async_trait::async_trait;
use gw_types::{
    packed::{RefreshMemBlockMessage, RefreshMemBlockMessageUnion},
    prelude::{Builder, Entity, Reader},
};
use rdkafka::{
    consumer::{CommitMode, Consumer as RdConsumer, StreamConsumer},
    producer::{FutureProducer, FutureRecord},
    ClientConfig, Message,
};

use crate::sync::{mq::RefreshMemBlockMessageFacade, subscribe::SubscribeMemPoolService};

use super::{Consume, Produce};

pub(crate) struct Producer {
    producer: FutureProducer,
    topic: String,
}

impl Producer {
    pub(crate) fn connect(hosts: Vec<String>, topic: String) -> Result<Self> {
        let brokers = hosts.join(",");
        let producer: FutureProducer = ClientConfig::new()
            .set("bootstrap.servers", brokers)
            .set("message.timeout.ms", "5000")
            .set("enable.idempotence", "true")
            .create()?;
        // .create_with_context(ProducerContextLogger)?;
        Ok(Self { producer, topic })
    }
}

#[async_trait]
impl Produce for Producer {
    type Msg = RefreshMemBlockMessageUnion;

    async fn send(&mut self, message: Self::Msg) -> Result<()> {
        let msg = RefreshMemBlockMessage::new_builder().set(message).build();
        let bytes = msg.as_bytes();
        log::trace!("Producer send msg: {:?}", &bytes.to_vec());
        let message = RefreshMemBlockMessageFacade(bytes);

        let ts = SystemTime::now().duration_since(UNIX_EPOCH)?.as_millis() as i64;
        let record = FutureRecord::to(&self.topic)
            .key("")
            .payload(&message)
            .timestamp(ts);
        if let Err((err, _)) = self.producer.send(record, Duration::from_millis(0)).await {
            log::error!("[kafka] send message failed: {:?}", &err);
        }
        Ok(())
    }
}

pub(crate) struct Consumer {
    consumer: StreamConsumer,
    subscribe: SubscribeMemPoolService,
    topic: String,
}

impl Consumer {
    pub(crate) fn start(
        hosts: Vec<String>,
        topic: String,
        group: String,
        subscribe: SubscribeMemPoolService,
    ) -> Result<Self> {
        let brokers = hosts.join(",");
        let consumer: StreamConsumer = ClientConfig::new()
            .set("bootstrap.servers", brokers)
            .set("session.timeout.ms", "6000")
            .set("enable.auto.commit", "false")
            .set("auto.offset.reset", "earliest")
            .set("group.id", group)
            .create()?;
        Ok(Self {
            consumer,
            topic,
            subscribe,
        })
    }
}

#[async_trait]
impl Consume for Consumer {
    async fn poll(&mut self) -> Result<()> {
        self.consumer.subscribe(&[&self.topic])?;
        while let Some(msg) = self.consumer.stream().next().await {
            match msg {
                Ok(msg) => {
                    let topic = msg.topic();
                    let partition = msg.partition();
                    let offset = msg.offset();
                    let payload = msg.payload();
                    let now = SystemTime::now().duration_since(UNIX_EPOCH)?.as_millis() as i64;
                    let msg_age = msg.timestamp().to_millis().map(|then| now - then);
                    log::debug!("kafka msg age: {:?}", msg_age);
                    log::trace!(
                        "Recv kafka msg: {}:{}@{}: {:?}",
                        topic,
                        partition,
                        offset,
                        &payload
                    );
                    if let Some(payload) = payload {
                        let refresh_msg = RefreshMemBlockMessage::from_slice(payload)?;
                        let reader = refresh_msg.as_reader();
                        let refresh_msg = reader.to_enum();
                        match &refresh_msg {
                        gw_types::packed::RefreshMemBlockMessageUnionReader::NextL2Transaction(
                            next,
                        ) => {
                            if let Err(err) = self.subscribe.next_tx(next.to_entity()).await {
                                log::error!("[Subscribe tx] error: {:?}", err);
                            }
                        }
                        gw_types::packed::RefreshMemBlockMessageUnionReader::NextMemBlock(next) => {
                            match self.subscribe.next_mem_block(next.to_entity()).await {
                                Ok(None) => {
                                    log::debug!("Invalid tip. Wait for syncing to the new tip.");
                                    //Postpone this message, consume it later.
                                    return Ok(());
                                }
                                Ok(Some(block_number)) => {
                                    log::debug!("Refresh mem pool to {}", block_number);
                                }
                                Err(err) => {
                                    log::error!("[Refresh mem pool] error: {:?}", err);
                                }
                            };
                        }
                    };
                        self.consumer.commit_message(&msg, CommitMode::Async)?;
                        log::trace!("Kafka commit offset: {}", offset);
                    };
                }
                Err(err) => {
                    log::error!("Receive error from kafka: {:?}", err);
                }
            }
        }
        Ok(())
    }
}
