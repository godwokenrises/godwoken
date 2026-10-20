# Godwoken withdraw

Rust CLI, refund-only V0/V1 contracts and a mainnet ETH OmniLock wallet. The wallet displays current on-chain assets and transactions without a refund table or protocol selector. Operator settlement commands still select V0 or V1 explicitly.

- [Current documents and complete tables](docs/README.md)
- [Operator runbook](docs/ops-refund-runbook.md)
- [Contract design](docs/contract-design.md)
- [Verification and limitations](docs/validation.md)
- [Web app](claim-web/README.md)
- [Wallet Docker deployment](docs/wallet-deployment.md)

From this directory, build with `cargo +1.95.0 build --locked --release --bin gw-withdraw`. Use the compiled executable's `--help` for exact flags.

```sh
gw-withdraw refund generate --protocol v1 --db /path/to/db --output-dir ./new-refunds
gw-withdraw l2-db --protocol v1 --db /path/to/db --output ./l2.json
gw-withdraw l1-custodian --protocol v1 --output ./l1.json
gw-withdraw check-backing --l2-report ./l2.json --l1-report ./l1.json --output ./backing.json
gw-withdraw wallet-serve --web-dir ./claim-web/dist --enable-broadcast
```

Use explicit `--protocol v0` for V0. Generation takes only a read-only DB and embedded reviewed policy; independent asset queries do not generate DeFi entitlements. Outputs refuse overwrite. Frozen L2 state and fresh L1 observations have separate anchors and scope limitations.

Operator commands are `refund`, `l2-db`, `l1-custodian`, `check-backing`, `upgrade`, `settlement` and the legacy release API `claim-serve`. Users receive and transfer ordinary CKB mainnet OmniLock funds through `wallet-serve`, including manual deposits and refunds. The refund lifecycle is one Begin, then Pay for selected applicants. Old per-record registration, direct L2-balance withdrawal and frozen-table research tools are not operator interfaces.

Publication, multisig upgrades, controller approvals and broadcasts follow the runbook; generating tables does not execute payments. Keep machine-specific paths and connection details in the separate machine inventory.

Historical table-driven research calculators have been removed. The two retained [developer diagnostics](examples/README.md) do not generate refunds or replace the CLI. The CLI and both network validators support only settlement V3: one state root, Begin and Pay. Standalone direct-account construction and its validator/test entrypoints have been removed.
