# Godwoken Refund deployment

The Docker image contains the `gw-withdraw wallet-serve` backend and the built
Godwoken Refund website. It needs a CKB mainnet RPC with Indexer APIs and a browser
with an Ethereum wallet. Refund tables, Godwoken databases, signing keys and
persistent volumes are not needed. The bundled OmniLock/sUDT dependencies already
exist on mainnet; deploying the website does not deploy any contracts.

## Build

Use Docker with BuildKit. Run from the **repository root**, not `exit-withdraw/`,
because the CLI has Rust dependencies in `gwos/` and `crates/`:

```sh
docker build -f exit-withdraw/Dockerfile -t godwoken-refund:local .
```

The first build compiles RocksDB and the Rust CLI and takes longer than subsequent
builds. BuildKit caches npm packages, Cargo downloads and native build artifacts.
Rust 1.95.0 and Node 24.21.0 are used only in build stages; the runtime image uses
Debian Bookworm and runs as UID/GID 10001. Its default command enables relaying
only user-signed transfers. It never holds a private key.

For a smaller build machine, add `--build-arg CARGO_BUILD_JOBS=2`. By default the
image targets the builder's architecture. To deploy to an x86 Linux server from
an ARM machine, build with `--platform linux/amd64` (emulation is slower), or build
on the target architecture. An ARM server uses `--platform linux/arm64`.

## Run locally

```sh
docker run -d --name godwoken-refund \
  --restart unless-stopped \
  --read-only \
  -p 127.0.0.1:8787:8787 \
  godwoken-refund:local
```

Open **http://127.0.0.1:8787**. Use this exact origin; `localhost` and `127.0.0.1`
are distinct hosts for the API. If another service already uses port 8787, change
both the host port and `--public-origin`, for example:

```sh
docker run -d --name godwoken-refund \
  --restart unless-stopped --read-only \
  -p 127.0.0.1:8788:8787 \
  godwoken-refund:local \
  --public-origin http://127.0.0.1:8788 --enable-broadcast
```

Arguments after the image name **replace** the default arguments. Include
`--enable-broadcast` when overriding arguments if users should be able to submit
their signed transfers. Omitting it leaves lookup and transaction review available
but disables submission.

Check readiness and logs:

```sh
curl --fail http://127.0.0.1:8787/api/wallet/config
docker logs --tail 100 godwoken-refund
```

The configuration response should identify `CKB mainnet`, a fee rate of `1000`,
and `broadcast_enabled: true` for the default command. This readiness check does
not send a transaction. Connect an ETH wallet to obtain the receiving address;
an actual withdrawal still requires the user to review and sign.

## Public HTTPS deployment

Run behind an HTTPS reverse proxy, for example Caddy. Replace the example domain
with your own domain and point its DNS at the server:

```sh
docker run -d --name godwoken-refund \
  --restart unless-stopped --read-only \
  -p 127.0.0.1:8787:8787 \
  godwoken-refund:local \
  --public-origin https://refund.example.com --enable-broadcast
```

If Caddy runs directly on the same host, this Caddyfile obtains HTTPS and forwards
requests while preserving the public Host and Origin headers:

```caddyfile
refund.example.com {
    reverse_proxy 127.0.0.1:8787
}
```

Serve the frontend and API on this same origin. Do not rewrite the upstream Host
to `127.0.0.1:8787`; the service checks Host against `--public-origin` and checks
Origin on submissions. Keep the backend port bound to loopback as above. If the
proxy itself runs in another container, use a shared Docker network and the
backend container name instead of the host-loopback proxy target.

## RPC and other settings

The default RPC and Indexer endpoint is `https://mainnet.ckb.dev/rpc`. Supply a
different mainnet provider if needed:

```sh
docker run -d --name godwoken-refund \
  --restart unless-stopped --read-only \
  -p 127.0.0.1:8787:8787 \
  godwoken-refund:local \
  --public-origin https://refund.example.com \
  --ckb-rpc https://YOUR_MAINNET_RPC \
  --indexer-rpc https://YOUR_MAINNET_INDEXER \
  --enable-broadcast
```

Omit `--indexer-rpc` when the RPC also serves Indexer methods. The provider must
support `get_cells`, `get_transactions`, historical transactions and headers,
`get_live_cell`, `estimate_cycles` and `send_transaction`. Transfers verify the
CKB mainnet genesis hash and the pinned dependency data hashes.

Defaults are one committed block for confirmations and 1000 shannons/KB for the
network fee, with no service fee. Change them with `--confirmations` and
`--fee-rate` if needed; the exact fee is shown before signing and is capped at
0.01 CKB. Unknown standard sUDTs are displayed in raw units. Other cell types
remain visible and require their asset-specific transfer tools.

Use `docker run --rm godwoken-refund:local --help` for available wallet flags.
No Node, npm, Cargo, CKB CLI or database server is required on the runtime host.

## Update or stop

Rebuild from the desired source, then recreate the container with the same
settings. There is no application data volume to migrate:

```sh
docker build -f exit-withdraw/Dockerfile -t godwoken-refund:local .
docker stop godwoken-refund
docker rm godwoken-refund
# Repeat the appropriate docker run command above.
```

Stopping or replacing the service does not move any funds. Assets remain in the
owner's on-chain OmniLock cells. Check any pending transaction by its existing
hash on Nervos Explorer before submitting another withdrawal.
