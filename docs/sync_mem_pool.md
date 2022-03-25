# sync mem block

We can scale up our handling ability by syncing mem block from the full node to multiple `ReadOnly` nodes.
So that requests like `/execute_raw_l2transaction` can be accessed just on read-only nodes.

## With P2P network

Configure listen and dial addresses of the full node and `ReadOnly` nodes so that all `ReadOnly` nodes are connected to the full node.

TODO: describe the MultiAddr format.

TODO: config example.

TODO: describe the protocol and how it works.

## With Kafka (legacy method)

### setup kafka locally

Setup your environment with kafka [quickstart](https://kafka.apache.org/quickstart).

Important steps:

- cd kafka/
- bin/zookeeper-server-start.sh config/zookeeper.properties # start zookeeper
- bin/kafka-server-start.sh config/server.properties # start kafka
- bin/kafka-topics.sh --create --partitions 1 --replication-factor 1 --topic **sync-mem-block** --bootstrap-server localhost:9092 # create our topic

### enable publishing mem block on full node

config.toml:

```toml
[mem_pool.publish]
hosts = ['localhost:9092']
topic = 'sync-mem-block'
```

### enalbe subscribing mem block on readonly node

config.toml:

```toml
[mem_pool.subscribe]
hosts = ['localhost:9092']
topic = 'sync-mem-block'
group = 'sync-mem-block-1'
```