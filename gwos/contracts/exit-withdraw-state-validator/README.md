# Godwoken refund state validators

Production refund binaries are `settlement-state-validator` for mainnet V1 and `v0-settlement-state-validator` for mainnet V0. Each pins its own approved profile. Settlement v2 names the action format, not a network version.

Both versions accept one Begin and subsequent epoch-0 Pay actions only. Ordinary rollup operations, legacy registration and later epochs are rejected. The native rollup account root commits settlement metadata; source/lot/claim membership, native balances, cumulative payments, custodian conservation and recipient authorization are checked together.

EOAs receive exact standard mainnet ETH OmniLock outputs. Approved control accounts use their committed distinct-owner thresholds and jointly authorize a common CKB destination. V0 authenticates ETH owners separately from L2 short addresses and preserves native shannon units.

- [Current contract design and encoding](../../../exit-withdraw/docs/contract-design.md)
- [Production build, upgrade and refund runbook](../../../exit-withdraw/docs/ops-refund-runbook.md)
- [Executed tests and limitations](../../../exit-withdraw/docs/validation.md)
- [Mainnet OmniLock deployment and source pages](../../../exit-withdraw/docs/v1/protocol-details.md)

Use the profile-pinned production build procedure in the runbook. Rust regression scripts under `exit-withdraw/scripts/` prepare synthetic profiles and build the required VM binaries; they are not deployment releases. Historical mainnet script inputs now live under `exit-withdraw/tests/fixtures/mainnet-evidence/`.

The tag-4 `exit-withdraw-state-validator` binary is retained for historical direct-account regression tests. It requires the explicit `legacy-tests` feature and is excluded from default builds. Regression scripts request it explicitly; production builds select only the V0/V1 settlement targets. It is not the allocation-based refund release or the operator upgrade target. No mainnet deployment is performed by building or testing this crate.
