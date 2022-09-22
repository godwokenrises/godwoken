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

### Authentication

Authentication between p2p peers is supported. Each node has a secp256k1
keypair, and a peer id that is derived from the public key. A node can be
configured to only connect to or allow peers with designated peer ids.

To use this authentication scheme, generate a secret key on each node:

```cmd
$ godwoken peer-id gen --secret-path s1
```

Calculate the corresponding peer id:

```cmd
$ godwoken peer-id from-secret --secret-path s1
QmTUDzfoDrEd6tB2qXHuVeqT7x9gWSrLgPQVD2wBGywtit
```

In the configuration of this node, set `secret_key_path`:

```toml
[p2p_network_config]
secret_key_path = "s1"
```

Then in the configuration of the peer node, add peer-id to `dial` or
`allowed_peer_ids`:

```toml
[p2p_network_config]
# Dial a peer node with peer id authentication.
dial = ["/dns4/godwoken/tcp/9999/p2p/QmTUDzfoDrEd6tB2qXHuVeqT7x9gWSrLgPQVD2wBGywtit"]
# Or for listening, only allow peers with these peer ids.
allowed_peer_ids = ["QmTUDzfoDrEd6tB2qXHuVeqT7x9gWSrLgPQVD2wBGywtit"]
```
