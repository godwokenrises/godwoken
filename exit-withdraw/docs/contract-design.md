# Root-only, applicant-driven refunds (V0 and V1)

Updated 16 October 2026: settlement **V3** uses one physical settlement SMT with bounded account pages. V0/V1 here identify native networks; V3 identifies the refund protocol. Approved amounts, identities and authority checks are preserved. Regenerate and verify the plan/profile before building the profile-pinned validator. Only V3 plans, profiles and actions are supported; no compatibility decoder or migration path is retained. Development tests do not upgrade or pay on mainnet.

## On-chain lifecycle

1. Publish the complete table, policy, plan and profile for independent checking;
   deploy/upgrade the profile-pinned validator using the existing authority.
2. One **Begin** transaction updates the rollup account SMT with metadata committing
   the profile hash and one settlement `state_root`. The profile commits the initial
   static account-page root and asset ceilings. Begin also initializes asset
   paid counters. It neither pays nor enumerates all beneficiaries.
3. For an applicant, **Pay** verifies claim, source and payable-lot membership,
   the actual native identity and balance, the recipient lock/controller authority,
   cumulative paid values, lot remainder, source budget and global asset budget.
   It transfers the requested assets from custodians and updates the native SMT.
4. Later Pay transactions repeat against current state. Non-applicants need no
   on-chain records or transactions. No registration/activation batch loop remains.

The rollup cell retains its original GlobalState format. Its account root commits
settlement metadata, which commits the profile and a single settlement state root; immutable source/lot/claim
records and their mutable counters share account pages in that tree. The account root is not overwritten with
the settlement state root. Old and new native SMT roots and one touched-state multiproof are
verified. An authenticated account page also proves which of its counter slots are absent.

Only Begin and epoch-0 Pay are accepted by the release. Legacy registration,
seal/activate, later epochs, cancellation and all ordinary rollup actions are
rejected. A changed table or previously retained balance cannot be introduced by
an arbitrary administrator transaction. Such policy changes need a separate review.

## Per-payment constraints

- Claim amount <= committed cap minus cumulative paid; duplicate claim keys in
  one action are rejected, and paid values survive process restarts.
- Source identity/type/baseline comes from its immutable account-page record, and native
  balance must equal baseline minus already paid before deducting the new payment.
- Lot must belong to the source, have kind INITIAL and contain sufficient remaining
  amount. A lot shared by claims cannot be reused beyond its total; deferred,
  excluded and protected lots cannot fund a payment. V0 protected raw destinations
  remain forbidden even if a malformed release attempted to make them payable.
- Source aggregate and asset aggregate remain bounded; custodian input assets equal
  retained custodian output assets plus exact payout assets. External governance
  inputs fund the operator fee. Minimum occupied CKB still applies to every output.
- EOAs receive standard ETH OmniLock outputs. Controller groups approve the full
  transaction through distinct actual OmniLock inputs returned unchanged.

The complete table's global partition/accounting is independently checked during
DB export and plan verification before root publication. The contract no longer
reconstructs the complete table by spending thousands of registration transactions;
it enforces the approved root and all relevant local/aggregate spending bounds.

## CLI and continuation

The production command names do not change: `refund export-plan`,
`settlement create-profile`, validator build/upgrade, then one `build-next`
without selections, followed by `build-next --addresses`, `--payments` or
`--controls`. See [Ops runbook](ops-refund-runbook.md).

After each confirmed transaction, use its `next-deployment-after-confirmation.json`
for fresh sponsor outpoints. Preserve the original DB, plan and deployment ID.
Every invocation walks confirmed rollup history, rebuilds state, checks the live
root and produces fresh SMT proofs. Do not process competing payouts against the
same rollup input. A user's subsequent L1 OmniLock spend does not reset entitlement
or alter the rollup. The archive RPC/witness availability requirement remains.

## Cold-process rehearsal

`settlement rehearse-next` exercises the production action/cell builder and shared
consensus checks against a real read-only DB, while storing a **synthetic** L1
history. It never signs, executes CKB-VM, calls network RPC or broadcasts. It must
not be confused with `verify-signed` or a deployable release. Its synthetic archive
contains transactions and release data, not saved SMT nodes or trusted paid counters.

```sh
gw-withdraw settlement --protocol v0 rehearse-next --db /path/to/original-v0-db \
  --plan full-plan.json --deployment rehearsal-deployment.json \
  --snapshot rehearsal-snapshot.json --output-dir step-0
# Exit the process. The next invocation reopens DB and reconstructs from transactions.
gw-withdraw settlement --protocol v0 rehearse-next --db /path/to/original-v0-db \
  --plan full-plan.json --archive step-0/archive.json \
  --payments first-partial.json --output-dir step-1
gw-withdraw settlement --protocol v0 rehearse-next --db /path/to/original-v0-db \
  --plan full-plan.json --archive step-1/archive.json \
  --addresses same-applicant.json --output-dir step-2
```

Use a new output directory each time. `summary.json` records before/after roots,
replayed history length, per-claim paid/remainder, custody coverage and unchanged
original DB root. The complete original DB test complements actual signed VM
fixtures, which use public test keys to cover bootstrap authority, EOA/controller
payouts, repeated payments, malicious proofs and onward OmniLock withdrawal.


## Exact custody and recipient checks

The shared verifier authenticates custodian code/hash type, rollup binding and finality. V1 retained custody uses the exact pinned change lock; V0 retained custody must pass native validation and match a consumed custodian lock. Inputs equal retained outputs plus payouts per exact asset type, including all CKB capacity. Governance funding pays the fee separately. Every output meets occupied capacity.

An EOA identity is authenticated from native state. Its output must be standard mainnet ETH OmniLock, type hash, exactly `0x01 || ETH[20] || 0x00`; extra arguments or another address fail. V0 short addresses are not substituted for ETH owners.

Controller identities and thresholds are root-committed descriptors. Distinct owners consume plain ETH OmniLock authorization cells and receive them unchanged. Real lock signatures bind the transaction. All payouts for one controller group use one jointly authorized CKB destination; it need not itself be an ETH OmniLock.

Code: [shared verifier](../../gwos/contracts/exit-withdraw-state-validator/src/verify.rs), [controller authorization](../../gwos/contracts/exit-withdraw-state-validator/src/control.rs), [settlement proofs](../../gwos/contracts/exit-withdraw-state-validator/src/settlement/mod.rs). The old and new native roots, claim/source/lot proofs and cumulative state are enforced by consensus, not only the frontend.

## Merkle records and reproducible encoding

| Record | Committed content |
|---|---|
| Source | Native account ID, script hash, address, asset ID, L1 type hash, baseline amount, evidence hash |
| Lot | Lot ID, source key, disposition kind, amount, evidence and reopening commitment |
| Claim | Epoch, source and lot keys, recipient ID/script hash/address, maximum amount, evidence hash |
| Source/asset budgets | Cumulative paid amount; immutable source/limit is authenticated separately |
| Reserve / Paid | Remaining lot amount / cumulative amount for a claim |
| Epoch | Claim count, manifest/policy and status; only initial epoch is payable |
| Metadata | Deployment/profile/snapshot, sequence and one settlement state root; committed as a native SMT leaf |

Records use exact tagged layouts and little-endian integers; trailing bytes are rejected. Nonempty leaf values are CKB-personalized Blake2b-256 of `GW_SETTLEMENT_V3/value\0 || encoded_record`; absent values are zero. Keys use `GW_SETTLEMENT_V3/key\0 || domain_length_u32_le || domain || deployment_id || record_identity`. The bytes `\0` here denote one NUL byte. Record identity and tags are specified by [records.rs](../../gwos/contracts/exit-withdraw-state-validator/src/settlement/records.rs); independent verifiers must use those exact bytes, not JSON serialization.

Source, lot and claim records are immutable portions of account pages. There are no independent source/lot/claim SMT roots in V3. Physical keys have canonical namespaces for `(account_id, page_number)`, asset ID and epoch ID. Each frozen page contains at most 16 static records and their paired counter slots, with at most 32 entries and 8 KiB total. Large accounts use multiple pages; subsequent payments never relocate a slot. Original full logical keys and full lot IDs are retained.

`Trees::set` allows only `PAID`, `RESERVES`, `SOURCES`, `ASSETS`; epoch initialization is allowed only during Begin. Clients submit payment references and amounts, not a key/value write list. The evaluator derives changes from authenticated old pages and preserves every other entry. The claimed output root must equal the root recomputed from exactly these derived changes. Native identity/storage write restrictions remain enforced separately.

Chain verification uses unmodified sparse-merkle-tree 0.6.1's C backend for compiled proof bytes. Rust verification/generation uses the upstream Rust implementation. Release builds use opt-level 3 for VM cycles; no SMT algorithm or opcode is patched.

Large actions are split before signing into 16,000-byte extra witnesses. A bounded length/count/full-payload-hash header occupies the action witness. Both V0 and V1 reconstruct it, and standard sighash signatures cover extra witnesses. Splitting witnesses solves a per-witness constraint; it does not remove transaction cycles or size limits.

The protocol permits at most 64 payment records and 20 recipients; this is not a guarantee that every combination fits a node's budget. Generate the full signed transaction and pre-execute it under the target node policy. Prefer grouping a recipient's CKB and tokens so token cells have enough capacity. The typical 70M cycles admission policy is a configurable node limit, not a consensus-wide per-transaction cap.

[Batch measurements and reproducible tests](validation.md) record tested batch sizes, failed single-transaction combinations and paging coverage.

The native payload is a version-3 `NativeTransition` table embedded in the settlement action. There is no standalone direct-account action envelope or verifier entrypoint. The eight logical record kinds are the complete set; obsolete registration slots, permit records, pending-epoch fields and reconstructed mutable-record copies are removed. Metadata contains only deployment/profile/snapshot, sequence and `state_root`.

CLI signing skeletons also require version 3. `verify-plan`, `create-profile`, `inspect-profile`, `build-next`, offline signing/assembly, `verify-signed`, status, portal proposals and confirmed-history replay all use this same format. V0 profiles start with `GWV0SET3`; V1 profiles use the V3 Molecule profile table. Native chain snapshots and CKB lock encodings retain their own formats.
