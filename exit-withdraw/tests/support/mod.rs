use godwoken_withdraw::{model::*, schema::*, store_serde};
use gw_common::state::*;
use gw_smt::{
    smt::{SMT, SMTH256},
    sparse_merkle_tree::default_store::DefaultStore,
};
use gw_types::{packed, prelude::*, registry_address::RegistryAddress, U256};
use rocksdb::{Options, DB};
use std::{collections::BTreeMap, path::Path};
pub const ETH: [u8; 20] = [0x12; 20];
pub fn amount(n: U256) -> [u8; 32] {
    let mut b = [0; 32];
    n.to_little_endian(&mut b);
    b
}
pub fn fixture(path: &Path, native_extra: u64, token_balance: u64) -> Bundle {
    fixture_address(path, native_extra, token_balance, ETH)
}
pub fn fixture_address(
    path: &Path,
    native_extra: u64,
    token_balance: u64,
    eth: [u8; 20],
) -> Bundle {
    fixture_addresses(path, native_extra, token_balance, &[eth])
}
pub fn fixture_addresses(
    path: &Path,
    native_extra: u64,
    token_balance: u64,
    addresses: &[[u8; 20]],
) -> Bundle {
    fixture_values(path, native_extra, token_balance, addresses, None)
}
pub fn fixture_values(
    path: &Path,
    native_extra: u64,
    token_balance: u64,
    addresses: &[[u8; 20]],
    values: Option<&[(U256, U256)]>,
) -> Bundle {
    fixture_configured(
        path,
        native_extra,
        token_balance,
        addresses,
        values,
        None,
        0,
    )
}
pub fn fixture_settlement(path: &Path, addresses: &[[u8; 20]]) -> Bundle {
    fixture_configured(path, 123, 500, addresses, None, None, 46)
}
pub fn fixture_missing_contract_code(path: &Path) -> Bundle {
    fixture_configured(path, 0, 500, &[ETH], None, Some([42; 32]), 0)
}
pub fn fixture_empty_contract_code(path: &Path) -> Bundle {
    fixture_configured(path, 0, 500, &[ETH], None, Some(hash(&[])), 0)
}
fn fixture_configured(
    path: &Path,
    native_extra: u64,
    token_balance: u64,
    addresses: &[[u8; 20]],
    values: Option<&[(U256, U256)]>,
    contract_code: Option<[u8; 32]>,
    extra_assets: usize,
) -> Bundle {
    if let Some(values) = values {
        assert_eq!(values.len(), addresses.len());
    }
    let evidence: serde_json::Value =
        serde_json::from_str(include_str!("../fixtures/mainnet-evidence/v1-config-cell.json")).unwrap();
    let config_data = evidence["response"]["result"]["cell"]["data"]["content"]
        .as_str()
        .unwrap()
        .to_owned();
    let cfg = Config::parse(&bytes(&config_data).unwrap()).unwrap();
    let prefix = hash32(ROLLUP_HASH).unwrap();
    let token = Script {
        code_hash: cfg.hashes["l1_sudt"].clone(),
        hash_type: "type".into(),
        args: hx([7; 32]),
    };
    let asset = token.hash().unwrap();
    let mut scripts: Vec<_> = (0..4 + addresses.len() + extra_assets)
        .map(|n| Script {
            code_hash: hx([(n + 20) as u8; 32]),
            hash_type: "type".into(),
            args: hx(prefix),
        })
        .collect();
    for (i, suffix) in [(1, [0; 32]), (3, hash32(&asset).unwrap())] {
        scripts[i] = Script {
            code_hash: cfg.hashes["l2_sudt"].clone(),
            hash_type: "type".into(),
            args: hx([prefix.as_slice(), suffix.as_slice()].concat()),
        };
    }
    for (index, eth) in addresses.iter().enumerate() {
        scripts[4 + index] = Script {
            code_hash: "0x096df264f38fff07f3acd318995abc2c71ae0e504036fe32bc38d5b6037364d4".into(),
            hash_type: "type".into(),
            args: hx([prefix.as_slice(), eth.as_slice()].concat()),
        };
    }
    for n in 0..extra_assets {
        scripts[4 + addresses.len() + n] = Script {
            code_hash: cfg.hashes["l2_sudt"].clone(),
            hash_type: "type".into(),
            args: hx([prefix.as_slice(), &[(128 + n) as u8; 32]].concat()),
        };
    }
    if contract_code.is_some() {
        let code = cfg
            .packed
            .allowed_contract_type_hashes()
            .into_iter()
            .find(|a| u8::from(a.type_()) == 3)
            .unwrap()
            .hash();
        scripts[4].code_hash = hx(code.as_slice());
        scripts[4].args = hx([prefix.as_slice(), &3u32.to_le_bytes(), ETH.as_slice()].concat());
    }
    let mut leaves: BTreeMap<[u8; 32], [u8; 32]> = BTreeMap::new();
    if contract_code.is_some() {
        let mut key = [0; 32];
        key[..4].copy_from_slice(&4u32.to_le_bytes());
        key[4] = 0xff;
        key[5] = 1;
        leaves.insert(build_account_key(4, &key), contract_code.unwrap());
    }

    for (i, s) in scripts.iter().enumerate() {
        leaves.insert(
            build_account_field_key(i as u32, GW_ACCOUNT_SCRIPT_HASH_TYPE),
            hash32(&s.hash().unwrap()).unwrap(),
        );
    }
    for (index, eth) in addresses.iter().enumerate() {
        let registry = RegistryAddress::new(2, eth.to_vec());
        let sh = hash32(&scripts[4 + index].hash().unwrap()).unwrap();
        leaves.insert(
            build_account_key(2, &build_registry_address_to_script_hash_key(&registry)),
            sh,
        );
        let mut reg = [0; 32];
        registry.write_to_slice(&mut reg).unwrap();
        leaves.insert(
            build_account_key(2, &build_script_hash_to_registry_address_key(&sh)),
            reg,
        );
        let mut id = [0; 32];
        id[..4].copy_from_slice(&((4 + index) as u32).to_le_bytes());
        id[4] = 1;
        leaves.insert(build_script_hash_to_account_id_key(&sh), id);
        let native = U256::from(1400) * U256::exp10(18) + U256::from(native_extra);
        let (native_balance, token_value) = values
            .map(|v| v[index])
            .unwrap_or((native, U256::from(token_balance)));
        let native_supply = values
            .map(|v| v.iter().map(|x| x.0).fold(U256::zero(), |a, b| a + b))
            .unwrap_or(native * U256::from(addresses.len()));
        let token_supply = values
            .map(|v| v.iter().map(|x| x.1).fold(U256::zero(), |a, b| a + b))
            .unwrap_or(U256::from(500 * addresses.len()));
        for (aid, supply, balance) in [
            (1, native_supply, native_balance),
            (3, token_supply, token_value),
        ] {
            leaves.insert(
                build_account_key(aid, &SUDT_TOTAL_SUPPLY_KEY),
                amount(supply),
            );
            leaves.insert(
                build_account_key(aid, &build_sudt_key(1, &registry)),
                amount(balance),
            );
        }
    }
    let mut tree: SMT<DefaultStore<SMTH256>> = SMT::default();
    tree.update_all(
        leaves
            .iter()
            .map(|(k, v)| ((*k).into(), (*v).into()))
            .collect(),
    )
    .unwrap();
    let root: [u8; 32] = (*tree.root()).into();
    let post = packed::AccountMerkleState::new_builder()
        .merkle_root(root.pack())
        .count(((4 + addresses.len() + extra_assets) as u32).pack())
        .build();
    let raw = packed::RawL2Block::new_builder()
        .number(20000u64.pack())
        .timestamp(123456u64.pack())
        .prev_account(post.clone())
        .post_account(post.clone())
        .build();
    let block_hash = hash(raw.as_slice());
    let block = packed::L2Block::new_builder().raw(raw).build();
    let mut opts = Options::default();
    opts.create_if_missing(true);
    opts.create_missing_column_families(true);
    let db = DB::open_cf(&opts, path, (0..COLUMNS).map(|n| n.to_string())).unwrap();
    let put = |col: usize, key: &[u8], val: &[u8]| {
        db.put_cf(db.cf_handle(&col.to_string()).unwrap(), key, val)
            .unwrap()
    };
    put(COLUMN_META, META_LAST_VALID_TIP_BLOCK_HASH_KEY, &block_hash);
    put(COLUMN_META, META_CHAIN_ID_KEY, &prefix);
    put(COLUMN_META, MIGRATION_VERSION_KEY, b"rust-fixture");
    put(COLUMN_BLOCK, &block_hash, block.as_slice());
    for (k, b) in tree.store().branches_map() {
        put(
            COLUMN_ACCOUNT_SMT_BRANCH,
            &store_serde::branch_key_to_vec(k),
            &store_serde::branch_node_to_vec(b),
        );
    }
    for (k, v) in tree.store().leaves_map() {
        put(COLUMN_ACCOUNT_SMT_LEAF, k.as_slice(), v.as_slice());
    }
    for s in &scripts {
        put(
            COLUMN_SCRIPT,
            &hash32(&s.hash().unwrap()).unwrap(),
            s.packed().unwrap().as_slice(),
        );
    }
    put(
        COLUMN_ASSET_SCRIPT,
        &hash32(&asset).unwrap(),
        token.packed().unwrap().as_slice(),
    );
    db.flush().unwrap();
    drop(db);
    let state = packed::GlobalState::new_builder()
        .rollup_config_hash(hash(&bytes(&config_data).unwrap()).pack())
        .account(post)
        .block(
            packed::BlockMerkleState::new_builder()
                .count(20001u64.pack())
                .build(),
        )
        .tip_block_hash(block_hash.pack())
        .tip_block_timestamp(123456u64.pack())
        .last_finalized_timepoint(3200u64.pack())
        .status(0u8.into())
        .version(1u8.into())
        .build();
    let dep = packed::DepositLockArgs::new_builder()
        .owner_lock_hash([0u8; 32].pack())
        .layer2_lock(scripts[4].packed().unwrap())
        .cancel_timeout(0u64.pack())
        .registry_id(2u32.pack())
        .build();
    let args = packed::CustodianLockArgs::new_builder()
        .deposit_block_hash([0u8; 32].pack())
        .deposit_finalized_timepoint(0u64.pack())
        .deposit_lock_args(dep)
        .build();
    let lock = Script {
        code_hash: cfg.hashes["custodian"].clone(),
        hash_type: "type".into(),
        args: hx([prefix.as_slice(), args.as_slice()].concat()),
    };
    let cell = |i, capacity: u64, token: Option<Script>, data: String| Cell {
        out_point: OutPoint {
            tx_hash: hx([9; 32]),
            index: format!("0x{i:x}"),
        },
        output: CellOutput {
            capacity: format!("0x{capacity:x}"),
            lock: lock.clone(),
            type_script: token,
        },
        output_data: data,
        block_number: "0x1".into(),
        tx_index: "0x0".into(),
    };
    let mut cells: BTreeMap<_, _> = FAMILIES
        .iter()
        .map(|f| (f.to_string(), Vec::new()))
        .collect();
    cells.get_mut("custodian").unwrap().extend([
        cell(
            0,
            1000 * 100_000_000 * addresses.len() as u64,
            None,
            "0x".into(),
        ),
        cell(
            1,
            400 * 100_000_000 * addresses.len() as u64,
            Some(token),
            hx((500u128 * addresses.len() as u128).to_le_bytes()),
        ),
    ]);
    Bundle {
        schema_version: 1,
        network: "mainnet_v1".into(),
        observed_start: "synthetic-test-only".into(),
        observed_end: "synthetic-test-only".into(),
        rollup_cell: Cell {
            out_point: OutPoint {
                tx_hash: hx([8; 32]),
                index: "0x0".into(),
            },
            output: CellOutput {
                capacity: "0x0".into(),
                lock: Script {
                    code_hash: hx([0; 32]),
                    hash_type: "data".into(),
                    args: "0x".into(),
                },
                type_script: Some(rollup_script()),
            },
            output_data: hx(state.as_slice()),
            block_number: "0x1".into(),
            tx_index: "0x0".into(),
        },
        config_data,
        cells,
        consistency: "synthetic_test_only".into(),
        live_cells_checked: 0,
        ckb_rpc: None,
        indexer_rpc: None,
        indexer_before: serde_json::Value::Null,
        indexer_after: serde_json::Value::Null,
    }
}
