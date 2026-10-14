# V1 identity, units and deployment

V1 ETH EOAs use authenticated registry identities. Recognized account lock args bind `rollup[32] || ETH[20]`. Native CKB uses 18 L2 decimals, converted to 8-decimal L1 shannons by dividing by 10^10 after beneficiary aggregation; fractional shannons remain reserved. Other tokens retain raw native quantities with verified display decimals.

The reviewed frozen L2 block is 2,851,258. Query outputs identify the exact account root, count and observed L1 rollup. Recorded supply, registered balance sums and additional discovered balances are distinct quantities; do not silently replace one with another.

Upgrade requires the live code cell's existing CKB multisig. Refund initialization and payments require separately pinned governance. Neither governance signature can override a committed EOA destination or controller threshold.

## Fresh L1 signatures

Both versions pay eligible ETH owners to the same standard mainnet ETH OmniLock. A fresh transaction-bound CKB signing digest is passed as 32 raw bytes to Ethereum `personal_sign`; the wallet adds the Ethereum prefix. Do not sign ASCII hex or reuse old L2 signatures. Normalize recovery IDs 27/28 to 0/1 and encode the OmniLock witness. TRON and unrecognized account modes remain outside this release.

## Selected mainnet parameters and source pages

| Parameter | Selected value |
| --- | --- |
| Network | CKB mainnet (Mirana in the RFC deployment table) |
| Lock `code_hash` | `0x9b819793a64463aed77c615d6cb226eea5487ccfc0783043a587254cda2b6f26` |
| Lock `hash_type` | `type` |
| Lock `args` | `0x01 || recipient_eth_address[20] || 0x00` (22 bytes) |
| CellDep `out_point.tx_hash` | `0xc76edf469816aa22f416503c38d0b533d2a018e253e379f134c3985b3472c842` |
| CellDep `out_point.index` | `0x0` |
| CellDep `dep_type` | `code` |
| Observed binary data hash (2026-09-22) | `0x768f306681da232ceb0b94f436c5f813377179762a831c5ad8797bd4fd2d118d` |

The binary data hash is for identifying the observed executable; it is **not** the `code_hash` of the selected `type` Script.

Where these values were found:

1. [Nervos RFC 0042 — Notes, Mirana deployment table](https://github.com/nervosnetwork/rfcs/blob/master/rfcs/0042-omnilock/0042-omnilock.md#notes): publishes the mainnet code hash, hash type, deployment transaction, output index and dependency type. The same section identifies reproducible-build source revision `cd764d`.
2. [Lumos predefined configuration — mainnet `LINA.SCRIPTS.OMNILOCK`, lines 68–76](https://github.com/ckb-js/lumos/blob/b1ded0809d7b7b09c3ceafdf9cdb74256232c311/packages/config-manager/src/predefined.ts#L68-L76): independently supplies the same five deployment parameters. This link pins the inspected revision rather than following a moving branch.
3. [OmniLock identity source — Ethereum flag and verification](https://github.com/nervosnetwork/omnilock/blob/280dd97ac0930ea1ed766a18d49478229347c832/c/ckb_identity.h) and [args parser](https://github.com/nervosnetwork/omnilock/blob/280dd97ac0930ea1ed766a18d49478229347c832/c/omni_lock.c): establish `0x01` Ethereum identity, 20 address bytes and the final feature byte. The recipient-dependent args are constructed from this format, not copied from the deployment table.
4. [Lumos OmniLock + MetaMask example](https://github.com/ckb-js/lumos/blob/b1ded0809d7b7b09c3ceafdf9cdb74256232c311/examples/omni-lock-metamask/lib.ts): demonstrates the browser signing and witness integration. See the parameter-order caveat below.
5. [Saved mainnet RPC response](../../tests/fixtures/mainnet-evidence/eth-lock-research/standard-omnilock-live.json): provides the observed liveness, binary bytes and data hash. These observations came from `get_live_cell` on `https://mainnet.ckb.dev/rpc`, not from the RFC deployment table.

The selected OmniLock dependency is only one dependency in a claim transaction. Include the network's secp256k1 data dependency/dep group and the appropriate token script dependencies as described below. Refresh live dependency outpoints before signing; a `type` reference follows the TYPE_ID code lineage and does not make the observed executable immutable.


[Operator procedures](../ops-refund-runbook.md) · [Current contract](../contract-design.md). Deployment values are pinned references; check actual live cells/data hashes before publishing any transaction. Mainnet script test inputs are under `../../tests/fixtures/mainnet-evidence/`.

## State and claim separation

The original DB remains frozen. After initialization or a payout, settlement history reconstructs the new on-chain account root and paid counters. Re-running a frozen DB inventory alone does not identify the amount still payable. Use settlement status with confirmed L1 history before each batch; retain the plan and original DB unchanged. This documentation refresh did not deploy or pay a refund.
