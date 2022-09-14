# P2P syncing

We can scale up our handling ability by syncing blocks and pending transactions from the full node to multiple `ReadOnly` nodes.
So that requests like `/execute_raw_l2transaction` can be accessed just on read-only nodes.

## Configuration

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

TODO: secret key and peer id
