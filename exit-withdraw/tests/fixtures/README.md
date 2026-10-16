# Additional mainnet VM fixtures

Captured on 2026-09-22 for offline integration tests. These fixtures contain public code/cell metadata, not credentials. Tests synthesize funding/deployment transactions and do not use the saved mainnet outpoints as live inputs.

- `mainnet-sudt-live.json`: raw JSON-RPC response from `https://mainnet.ckb.dev/rpc`, method `get_live_cell`, params `[{"tx_hash":"0xc7813f6a415144643970c2e88e0bb6ca6a8edc5dd7c1022746f628284a9936d5","index":"0x0"},true]`. The sUDT type hash is `0x5e7a36a77e68eecc013dfa2fe6a23f3b6c344b04005808694ae6dd45eea4cfd5`. Deployment reference: [Lumos mainnet predefined configuration](https://github.com/ckb-js/lumos/blob/develop/packages/config-manager/src/predefined.ts), `LINA.SCRIPTS.SUDT`.
- `mainnet-multisig-type.json`: extracted outpoint and type Script from mainnet genesis (`get_block_by_number("0x0")` on the same RPC), transaction 0/output 4. Its type hash is the standard legacy multisig `0x5c5069eb0857efc65e1bca0c07df34c31663b3622fd3876c876320fc9634e2a8`. Tests load the bytecode and secp data from pinned `ckb-system-scripts` 0.5.4, not from an always-success replacement.

SHA256 of the saved files:

```text
db77b93abd9ba8ead8913daf10d3d0c1d353d32e335914afda9f37718ca1f3cf  mainnet-sudt-live.json
36f07dda7a29155ba7239bdfdf16b126d3623f69ca5bc2263df3e4de901ce9b7  mainnet-multisig-type.json
```

Other real mainnet code bytes are reused from `tests/fixtures/mainnet-evidence/`: custodian and validator type metadata, the original Godwoken OmniLock, and standard mainnet OmniLock. See [ETH lock research](../../docs/v1/protocol-details.md) for source pages and deployment hashes. V3 settlement regression scripts build the network-specific validator against a synthetic V3 profile.

Current allocation-based V0/V1 tests use the settlement test scripts documented in [validation](../../docs/validation.md). Mainnet observations and root-only VM evidence are test inputs, not current refund tables.
