# sync mem block

We can scale up our handling ability by syncing mem block from the full node to multiple `ReadOnly` nodes.
So that requests like `/execute_raw_l2transaction` can be accessed just on read-only nodes.

## With P2P networking

Configure listen and dial addresses of the full node and read-only nodes so that all read-only nodes are connected to the full node. There should be one and only one connection for each read-only node.

Like many other nervosnetwork projects, godwoken uses [tentacle](https://github.com/nervosnetwork/tentacle) for p2p networking, which uses the [MultiAddr](https://github.com/multiformats/multiaddr) format for addressing, so this is the format we use in configuration files too.

Here is an example configuration snippet for a full node:

```toml
node_mode = "fullnode"

[p2p_network_config]
listen = "/ip4/0.0.0.0/tcp/9999"
```

And for a read-only node:

```toml
node_mode = "readonly"

[p2p_network_config]
dial = ["/dns4/godwoken/tcp/9999"]
```

## With Kafka

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