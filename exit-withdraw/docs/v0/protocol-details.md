# V0 identity, units and deployment

V0 ETH owner `E` is the final 20 bytes of recognized account lock args `rollup[32] || E[20]`. The L2 short address is `CKBHash(Molecule(Script))[0..20]`, not `E`. Native balances use the short address and u128 amounts. Native CKB is already shannons (8 decimals); WCKB storage also uses shannons despite its displayed 18-decimal getter. The known DB tip 2,071,124 and L1 tip 2,071,125 share an account root through a verified empty successor; block histories must not be called identical.

Recognized ETH account lock code hash: `0x1563080d175bf8ddd44a48e850cecf0c0b4575835756eb5ffd53ad830931b9f9`, type. Rollup: `0x40d73f0d3c561fcaae330eabc030d8d96a9d0af36d0c5114883658a350cb9e3b`. [Historical native state helpers](https://github.com/godwokenrises/godwoken/blob/becf2bb48d3d49d39d1fc77b87f360511cee4c1e/crates/common/src/state.rs).

Upgrade uses the existing code cell multisig (observed 3-of-5), separate from beneficiary 2-of-3 policies. Bootstrap additionally requires both existing PoA producers and retires helper locks; subsequent refund actions use the pinned single-signature CKB governance authority. Resolve live cells and exact multisig preimages before execution.

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

Keep the frozen original DB unchanged after payouts. Fresh live rollup roots after Begin/Pay are reconstructed from the immutable plan and confirmed L1 history; they are not expected to equal the original pre-refund account root. Native asset queries describe that frozen DB. Use settlement status/replay for current paid state. The current documentation refresh occurred before any deployed refund, so the fresh DB/L1 comparison still uses the frozen root.
