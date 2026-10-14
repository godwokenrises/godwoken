# Godwoken refund withdraw portal

Shared React/TypeScript UI and Rust `gw-withdraw claim-serve` API. Users explicitly choose V1 (default) or V0 and Personal or Contract Account. Only the selected protocol is queried and signed. ETH owners are supported; TRON is deferred.

Personal mode displays published rights and funded standard ETH OmniLock cells. Users choose asset amounts and a valid CKB mainnet destination, then sign an L1 transaction with their ETH wallet. Partial withdrawals preserve change and allow later withdrawal of the remainder. Unfunded rights cannot be spent as live cells.

Contract mode reviews an Ops-prepared, immutable proposal, checks its committed owners/threshold and destination, and exchanges/imports distinct controller approvals. The API verifies the signed proposal and can submit the approved settlement transaction when broadcasting is enabled. Unlike personal L1 transfers, this path can consume the rollup/custodians through the refund contract. The service holds no private keys and does not open the L2 DB.

[Deployment and signing procedures](../docs/ops-refund-runbook.md#6-run-the-web-portal) · [Test coverage and boundaries](../docs/validation.md)

```sh
npm ci
npm test
npm run build
```

Run these in `claim-web/`. Export separate current portal packages with `gw-withdraw settlement --protocol v0|v1 export-portal`, then serve using the runbook. Do not point the service at an obsolete historical table directory. `--enable-broadcast` is opt-in and requires reviewed deployment dependencies and authorization.

To smoke-test two exported public packages, from any directory:

```sh
node /path/to/claim-web/scripts/smoke-protocols.mjs /path/to/portal-v0 /path/to/portal-v1
```

The smoke test starts a local keyless server with broadcast disabled and verifies version isolation and units. Browser rehearsals use injected wallets/API fixtures; they do not certify the actual MetaMask extension or mainnet submission.
