#![allow(dead_code)]
use crate::*;
use alloc::{vec, vec::Vec};
use gw_common::state::*;
use gw_hash::blake2b::hash;
use gw_smt::smt::{default_store::DefaultStore, SMT, SMTH256};
use gw_types::{packed, prelude::*, registry_address::RegistryAddress, U256};
use std::collections::{BTreeMap, BTreeSet};
pub const CKB: u64 = 100_000_000;
pub fn script(code: Hash, args: Vec<u8>) -> packed::Script {
    packed::Script::new_builder()
        .code_hash(code.pack())
        .hash_type(1u8.into())
        .args(args.pack())
        .build()
}
pub fn cell(cap: u64, lock: packed::Script, t: Option<packed::Script>, data: Vec<u8>) -> Cell {
    Cell {
        capacity: cap * CKB,
        lock,
        type_: t,
        data,
    }
}
pub fn state(c: &Cell) -> packed::GlobalState {
    packed::GlobalState::from_slice(&c.data).unwrap()
}
pub fn set_root(c: &mut Cell, root: Hash) {
    let s = state(c);
    c.data = s
        .clone()
        .as_builder()
        .account(s.account().as_builder().merkle_root(root.pack()).build())
        .build()
        .as_slice()
        .to_vec();
}
pub struct Fixture {
    pub p: Profile,
    pub cfg: packed::RollupConfig,
    pub tx: Transaction,
    pub a: Action,
    pub leaves: BTreeMap<Hash, Hash>,
    pub keys: BTreeSet<Hash>,
}
impl Fixture {
    pub fn new() -> Self {
        let rollup = script(script_hash(&code_type()), vec![88; 32]);
        let rh = script_hash(&rollup);
        let cfg = packed::RollupConfig::new_builder()
            .l1_sudt_script_type_hash([11; 32].pack())
            .custodian_script_type_hash(custodian_hash().pack())
            .l2_sudt_validator_script_type_hash([13; 32].pack())
            .finality_blocks(10u64.pack())
            .allowed_eoa_type_hashes(
                vec![packed::AllowedTypeHash::new_builder()
                    .type_(1u8.into())
                    .hash([14; 32].pack())
                    .build()]
                .pack(),
            )
            .build();
        let gov = script(MULTISIG, vec![21; 20]);
        let boot = script([23; 32], vec![24; 20]);
        let token = script([11; 32], vec![77; 32]);
        let th = script_hash(&token);
        let rec: Vec<_> = (0..2)
            .map(|i| {
                let address = [(i + 1) * 17; 20];
                Recipient {
                    address,
                    id: 9 + i as u32,
                    script: script([14; 32], [rh.as_slice(), address.as_slice()].concat()),
                }
            })
            .collect();
        let deposit = packed::DepositLockArgs::new_builder()
            .registry_id(2u32.pack())
            .layer2_lock(rec[0].script.clone())
            .build();
        let ca = packed::CustodianLockArgs::new_builder()
            .deposit_lock_args(deposit)
            .build();
        let change = script(custodian_hash(), [rh.as_slice(), ca.as_slice()].concat());
        let assets = vec![
            Asset {
                id: 1,
                script: script([13; 32], [rh.as_slice(), [0; 32].as_slice()].concat()),
            },
            Asset {
                id: 3,
                script: script([13; 32], [rh.as_slice(), th.as_slice()].concat()),
            },
        ];
        let gs = packed::GlobalState::new_builder()
            .rollup_config_hash(hash(cfg.as_slice()).pack())
            .account(
                packed::AccountMerkleState::new_builder()
                    .count(100u32.pack())
                    .build(),
            )
            .block(
                packed::BlockMerkleState::new_builder()
                    .count(100u64.pack())
                    .build(),
            )
            .version(1u8.into())
            .build();
        let rollin = cell(
            800,
            boot.clone(),
            Some(rollup.clone()),
            gs.as_slice().to_vec(),
        );
        let mut rollout = rollin.clone();
        rollout.lock = gov.clone();
        let inputs = vec![
            rollin,
            cell(800, change.clone(), None, vec![]),
            cell(
                800,
                change.clone(),
                Some(token.clone()),
                200u128.to_le_bytes().to_vec(),
            ),
            cell(100, gov.clone(), None, vec![]),
        ];
        let outputs = vec![
            rollout,
            cell(600, change.clone(), None, vec![]),
            cell(
                600,
                change.clone(),
                Some(token.clone()),
                170u128.to_le_bytes().to_vec(),
            ),
            cell(
                200,
                eth_lock(rec[0].address),
                Some(token.clone()),
                10u128.to_le_bytes().to_vec(),
            ),
            cell(
                200,
                eth_lock(rec[1].address),
                Some(token),
                20u128.to_le_bytes().to_vec(),
            ),
            cell(99, gov.clone(), None, vec![]),
        ];
        let outpoints: Vec<_> = (0..inputs.len())
            .map(|n| {
                packed::OutPoint::new_builder()
                    .tx_hash([n as u8 + 1; 32].pack())
                    .build()
            })
            .collect();
        let p = Profile {
            rollup,
            config_hash: hash(cfg.as_slice()),
            bootstrap_outpoint: outpoints[0].clone(),
            bootstrap_capacity: inputs[0].capacity,
            bootstrap_lock: boot,
            bootstrap_state: gs,
            governance: gov,
            change,
            eoa_codes: vec![[14; 32]],
            catalogue: core::iter::once((1, [0; 32]))
                .chain(core::iter::once((3, th)))
                .chain((0..46u8).map(|i| {
                    (
                        30 + i as u32,
                        script_hash(&script([11; 32], vec![80 + i; 32])),
                    )
                }))
                .collect(),
        };
        let a = Action {
            recipients: rec,
            assets,
            payouts: vec![
                Payout {
                    output: 3,
                    recipient: 0,
                    asset: 3,
                },
                Payout {
                    output: 4,
                    recipient: 1,
                    asset: 3,
                },
            ],
            kv: vec![],
            proof: gw_smt::smt::CompiledMerkleProof(vec![]),
        };
        let mut f = Self {
            p,
            cfg,
            tx: Transaction {
                inputs,
                outputs,
                outpoints,
            },
            a,
            leaves: BTreeMap::new(),
            keys: BTreeSet::new(),
        };
        for r in &f.a.recipients {
            let sh = script_hash(&r.script);
            let reg = RegistryAddress::new(2, r.address.to_vec());
            let mut encoded = [0; 32];
            reg.write_to_slice(&mut encoded).unwrap();
            let mut id = [0; 32];
            id[..4].copy_from_slice(&r.id.to_le_bytes());
            id[4] = 1;
            for (k, v) in [
                (
                    build_account_field_key(r.id, GW_ACCOUNT_SCRIPT_HASH_TYPE),
                    sh,
                ),
                (
                    build_account_key(2, &build_registry_address_to_script_hash_key(&reg)),
                    sh,
                ),
                (build_script_hash_to_account_id_key(&sh), id),
                (
                    build_account_key(2, &build_script_hash_to_registry_address_key(&sh)),
                    encoded,
                ),
            ] {
                f.leaves.insert(k, v);
            }
            f.leaves.insert(
                balance_key(1, r.address),
                value(U256::from(1000) * U256::exp10(18) + U256::from(123)),
            );
            f.leaves
                .insert(balance_key(3, r.address), value(U256::from(100)));
        }
        for asset in &f.a.assets {
            f.leaves.insert(
                build_account_field_key(asset.id, GW_ACCOUNT_SCRIPT_HASH_TYPE),
                script_hash(&asset.script),
            );
        }
        f.leaves.insert(
            supply_key(1),
            value(U256::from(2000) * U256::exp10(18) + U256::from(246)),
        );
        f.leaves.insert(supply_key(3), value(U256::from(200)));
        f.keys = f.leaves.keys().copied().collect();
        // An unrelated, hidden storage leaf must remain committed across the transition.
        f.leaves.insert([0xee; 32], [0xdd; 32]);
        f.refresh();
        f
    }
    pub fn tree(&self) -> SMT<DefaultStore<SMTH256>> {
        let mut t = SMT::default();
        t.update_all(
            self.leaves
                .iter()
                .map(|(k, v)| ((*k).into(), (*v).into()))
                .collect(),
        )
        .unwrap();
        t
    }
    pub fn refresh_input(&mut self) {
        self.keys.insert(metadata_key(&self.p));
        let mut t = self.tree();
        let root: Hash = (*t.root()).into();
        set_root(&mut self.tx.inputs[0], root);
        self.p.bootstrap_state = state(&self.tx.inputs[0]);
        self.a.kv = self
            .keys
            .iter()
            .map(|k| (*k, *self.leaves.get(k).unwrap_or(&[0; 32])))
            .collect();
        let keys: Vec<_> = self.keys.iter().map(|k| (*k).into()).collect();
        self.a.proof = t.merkle_proof(keys.clone()).unwrap().compile(keys).unwrap();
    }
    pub fn refresh(&mut self) {
        self.refresh_input();
        let mut t = self.tree();
        let mut debits: BTreeMap<(usize, u32), U256> = BTreeMap::new();
        for p in &self.a.payouts {
            let c = &self.tx.outputs[p.output as usize];
            *debits.entry((p.recipient as usize, 1)).or_default() +=
                U256::from(c.capacity) * U256::from(SCALE);
            if p.asset != 1 {
                *debits.entry((p.recipient as usize, p.asset)).or_default() +=
                    U256::from_little_endian(&c.data);
            }
        }
        let mut sums = BTreeMap::<u32, U256>::new();
        for ((r, id), n) in debits {
            let k = balance_key(id, self.a.recipients[r].address);
            let v = amount(*self.leaves.get(&k).unwrap_or(&[0; 32]));
            t.update(k.into(), value(v - n).into()).unwrap();
            *sums.entry(id).or_default() += n;
        }
        for (id, n) in sums {
            let k = supply_key(id);
            let v = amount(*self.leaves.get(&k).unwrap_or(&[0; 32]));
            t.update(k.into(), value(v - n).into()).unwrap();
        }
        set_root(&mut self.tx.outputs[0], (*t.root()).into());
    }
    pub fn verify(&self) -> Result<Outcome> {
        verify_native(&self.p, &self.cfg, &self.tx, &self.a)
    }
}
pub fn encode_action(a: &Action) -> Vec<u8> {
    crate::codec::encode_action(a)
}
pub fn encode_profile(p: &Profile) -> Vec<u8> {
    crate::codec::encode_profile(p)
}
fn metadata_key(p: &Profile) -> Hash {
    crate::settlement::records::metadata_key([99; 32], script_hash(&p.rollup))
}
// Exercise shared native checks with an already evaluated V3 transition. This is not a public entrypoint.
pub fn verify_native(
    p: &Profile,
    cfg: &packed::RollupConfig,
    tx: &Transaction,
    a: &Action,
) -> Result<Outcome> {
    let mut payments = BTreeMap::<(u32, u32), U256>::new();
    for payout in &a.payouts {
        let Some(cell) = tx.outputs.get(payout.output as usize) else {
            continue;
        };
        *payments.entry((payout.recipient, 1)).or_default() +=
            U256::from(cell.capacity) * U256::from(SCALE);
        if payout.asset != 1 && cell.data.len() == 16 {
            *payments
                .entry((payout.recipient, payout.asset))
                .or_default() += U256::from_little_endian(&cell.data);
        }
    }
    let mut sources = BTreeMap::<(u32, [u8; 20]), U256>::new();
    for ((r, id), n) in &payments {
        if let Some(recipient) = a.recipients.get(*r as usize) {
            *sources.entry((*id, recipient.address)).or_default() += *n;
        }
    }
    let key = metadata_key(p);
    crate::verify::verify_transition(
        p,
        cfg,
        tx,
        a,
        &crate::verify::SettlementTransition {
            admin: false,
            reads: BTreeSet::from([key]),
            metadata_key: key,
            metadata_value: [0; 32],
            sources,
            payments,
        },
    )
}

pub fn code_type() -> packed::Script {
    script([93; 32], vec![94; 32])
}
impl Fixture {
    /// Apply one accepted batch, preserving the deployment profile, then prepare the next.
    pub fn advance(&mut self) {
        let profile = self.p.clone();
        self.advance_input();
        self.refresh();
        self.p = profile;
    }
    pub fn advance_input(&mut self) {
        let outcome = self.verify().unwrap();
        let profile = self.p.clone();
        for ((r, id), n) in outcome.debits {
            let key = balance_key(id, self.a.recipients[r as usize].address);
            self.leaves
                .insert(key, value(amount(self.leaves[&key]) - n));
            let key = supply_key(id);
            self.leaves
                .insert(key, value(amount(self.leaves[&key]) - n));
        }
        for i in 0..3 {
            self.tx.inputs[i] = self.tx.outputs[i].clone();
            self.tx.outpoints[i] = packed::OutPoint::new_builder()
                .tx_hash([100 + i as u8; 32].pack())
                .build();
        }
        self.tx.outputs[1].capacity -= 200 * CKB;
        self.tx.outputs[2].capacity -= 200 * CKB;
        let token = u128::from_le_bytes(self.tx.outputs[2].data.clone().try_into().unwrap());
        self.tx.outputs[2].data = (token - 30).to_le_bytes().to_vec();
        self.refresh_input();
        self.p = profile;
    }
}

pub fn custodian_hash() -> Hash {
    hex::decode("000f87062a2fe9bb4a6cc475212ea11014b84deb32e0375ee51e6ec4a553e009")
        .unwrap()
        .try_into()
        .unwrap()
}
impl Fixture {
    pub fn eight_recipients(&mut self) {
        let original = self.p.clone();
        let rh = script_hash(&self.p.rollup);
        for i in 2..8u8 {
            let address = [(i + 1) * 17; 20];
            let id = 9 + i as u32;
            let s = script([14; 32], [rh.as_slice(), &address].concat());
            let sh = script_hash(&s);
            let reg = RegistryAddress::new(2, address.to_vec());
            let mut encoded = [0; 32];
            reg.write_to_slice(&mut encoded).unwrap();
            let mut account = [0; 32];
            account[..4].copy_from_slice(&id.to_le_bytes());
            account[4] = 1;
            for (k, v) in [
                (build_account_field_key(id, GW_ACCOUNT_SCRIPT_HASH_TYPE), sh),
                (
                    build_account_key(2, &build_registry_address_to_script_hash_key(&reg)),
                    sh,
                ),
                (build_script_hash_to_account_id_key(&sh), account),
                (
                    build_account_key(2, &build_script_hash_to_registry_address_key(&sh)),
                    encoded,
                ),
                (
                    balance_key(1, address),
                    value(U256::from(1000) * U256::exp10(18) + U256::from(123)),
                ),
                (balance_key(3, address), value(100.into())),
            ] {
                self.leaves.insert(k, v);
                self.keys.insert(k);
            }
            let index = self.tx.outputs.len();
            let mut payout = self.tx.outputs[3].clone();
            payout.lock = eth_lock(address);
            self.tx.outputs.push(payout);
            self.a.recipients.push(Recipient {
                address,
                id,
                script: s,
            });
            self.a.payouts.push(Payout {
                output: index as u32,
                recipient: i as u32,
                asset: 3,
            });
        }
        for i in [1, 2] {
            self.tx.inputs[i].capacity = 4000 * CKB;
            self.tx.outputs[i].capacity = 3200 * CKB;
        }
        self.tx.inputs[2].data = 800u128.to_le_bytes().to_vec();
        self.tx.outputs[2].data = 710u128.to_le_bytes().to_vec();
        self.leaves.insert(
            supply_key(1),
            value(U256::from(8000) * U256::exp10(18) + U256::from(984)),
        );
        self.leaves.insert(supply_key(3), value(800.into()));
        self.tx.inputs[0].lock = self.p.governance.clone();
        self.refresh();
        self.p = original;
    }
}

#[path = "native_adversarial.rs"]
pub mod adversarial;

impl Fixture {
    pub fn add_background_state(&mut self, count: u32) {
        let profile = self.p.clone();
        for index in 0..count {
            let key = hash(&[b"cycle-stress-background".as_slice(), &index.to_le_bytes()].concat());
            self.leaves.insert(key, hash(&index.to_le_bytes()));
        }
        self.tx.inputs[0].lock = self.p.governance.clone();
        self.refresh();
        self.p = profile;
    }
}

impl Fixture {
    pub fn multiple_assets(&mut self, tokens: u8) {
        assert!((1..=40).contains(&tokens));
        let profile = self.p.clone();
        let rh = script_hash(&self.p.rollup);
        for i in 0..tokens - 1 {
            let id = 30 + i as u32;
            let token = script([11; 32], vec![80 + i; 32]);
            let asset = script([13; 32], [rh.as_slice(), &script_hash(&token)].concat());
            self.leaves.insert(
                build_account_field_key(id, GW_ACCOUNT_SCRIPT_HASH_TYPE),
                script_hash(&asset),
            );
            self.leaves.insert(supply_key(id), value(200.into()));
            self.keys
                .insert(build_account_field_key(id, GW_ACCOUNT_SCRIPT_HASH_TYPE));
            self.keys.insert(supply_key(id));
            self.a.assets.push(Asset { id, script: asset });
            self.tx.inputs.push(cell(
                800,
                self.p.change.clone(),
                Some(token.clone()),
                200u128.to_le_bytes().to_vec(),
            ));
            self.tx.outpoints.push(
                packed::OutPoint::new_builder()
                    .tx_hash([150 + i; 32].pack())
                    .build(),
            );
            self.tx.outputs.push(cell(
                400,
                self.p.change.clone(),
                Some(token.clone()),
                170u128.to_le_bytes().to_vec(),
            ));
            for (r, recipient) in self.a.recipients.iter().enumerate() {
                self.leaves
                    .insert(balance_key(id, recipient.address), value(100.into()));
                self.keys.insert(balance_key(id, recipient.address));
                let index = self.tx.outputs.len();
                self.tx.outputs.push(cell(
                    200,
                    eth_lock(recipient.address),
                    Some(token.clone()),
                    ((r as u128 + 1) * 10).to_le_bytes().to_vec(),
                ));
                self.a.payouts.push(Payout {
                    output: index as u32,
                    recipient: r as u32,
                    asset: id,
                });
            }
        }
        for r in &self.a.recipients {
            self.leaves.insert(
                balance_key(1, r.address),
                value(U256::from(10_000) * U256::exp10(18)),
            );
        }
        self.leaves
            .insert(supply_key(1), value(U256::from(20_000) * U256::exp10(18)));
        self.a
            .payouts
            .sort_by_key(|p| (p.recipient, p.asset, p.output));
        self.tx.inputs[0].lock = self.p.governance.clone();
        self.refresh();
        self.p = profile;
    }
}

pub fn maximum_cells_fixture() -> Fixture {
    let mut f = Fixture::new();
    for i in 0..MAX_INPUTS - f.tx.inputs.len() {
        let args = f.p.change.args().raw_data();
        let ca = packed::CustodianLockArgs::from_slice(&args[32..])
            .unwrap()
            .as_builder()
            .deposit_block_hash([i as u8 + 1; 32].pack())
            .build();
        let lock = script(custodian_hash(), [&args[..32], ca.as_slice()].concat());
        f.tx.inputs.push(cell(800, lock, None, vec![]));
        f.tx.outpoints.push(
            packed::OutPoint::new_builder()
                .tx_hash([100 + i as u8; 32].pack())
                .build(),
        );
        f.tx.outputs[1].capacity += 800 * CKB;
    }
    for _ in f.tx.outputs.len()..MAX_OUTPUTS {
        f.tx.outputs
            .push(cell(600, f.p.change.clone(), None, vec![]));
        f.tx.inputs[1].capacity += 600 * CKB;
    }
    f
}
