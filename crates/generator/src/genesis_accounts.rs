use crate::traits::StateExt;
use gw_common::{builtins::CKB_SUDT_ACCOUNT_ID, smt::H256, state::State};
use gw_traits::CodeStore;
use gw_types::{core::ScriptHashType, offchain::RollupContext, packed::Script, prelude::*};
use secp256k1::{rand::rngs::OsRng, Secp256k1};
use sha3::{Digest, Keccak256};

pub const GENESIS_ACCOUNT_SKS: &str = "/code/workspace/account_sks";
pub const GENESIS_ACCOUNTS: &str = "/code/workspace/accounts";

pub fn load_and_generate_genesis_accounts(
    state: &mut (impl State + StateExt + CodeStore),
    rollup_context: &RollupContext,
) {
    // Setup accounts for benchmark
    if std::fs::File::open(GENESIS_ACCOUNT_SKS).is_err() {
        generate_genesis_account_sks();
    }
    let allowed_eoa_type_hashes = rollup_context.rollup_config.allowed_eoa_type_hashes();
    let accounts = generate_genesis_accounts_with_state(
        state,
        &rollup_context.rollup_script_hash,
        &allowed_eoa_type_hashes.get(0).unwrap().unpack(),
    );
    log::info!("generate genesis accounts {}", accounts.len());
}

#[allow(dead_code)]
struct Account {
    id: u32,
    sk: [u8; 32],
    eth_addr: [u8; 20],
    script: Script,
}

fn generate_genesis_account_sks() {
    let accounts: u64 = {
        let accounts = std::fs::read_to_string(GENESIS_ACCOUNTS).unwrap();
        accounts.parse().unwrap()
    };

    let private_keys = (0..accounts)
        .map(|_| {
            let sk = secp256k1::SecretKey::new(&mut OsRng::new().unwrap());
            format!("0x{}", hex::encode(sk.as_ref()))
        })
        .collect::<Vec<_>>();

    std::fs::write(
        std::path::Path::new(GENESIS_ACCOUNT_SKS),
        private_keys.join("\n").as_bytes(),
    )
    .unwrap();

    log::info!("write account sks to {}", GENESIS_ACCOUNT_SKS);
}

fn generate_genesis_accounts_with_state(
    state: &mut (impl State + StateExt + CodeStore),
    rollup_type_hash: &H256,
    eth_account_lock_hash: &H256,
) -> Vec<Account> {
    const BENCH_GENESIS_ACCOUNT_CKB_BALANCE: u128 = 100_000_000;

    let secp = Secp256k1::new();
    let build_account = |hex: &str| -> _ {
        let decoded = hex::decode(hex.trim_start_matches("0x")).unwrap();
        let sk = secp256k1::SecretKey::from_slice(&decoded).unwrap();
        let pk = secp256k1::PublicKey::from_secret_key(&secp, &sk);

        let mut hasher = Keccak256::new();
        hasher.update(&pk.serialize_uncompressed()[1..]);
        let buf = hasher.finalize();

        let mut eth_addr = [0u8; 20];
        eth_addr.copy_from_slice(&buf[12..]);

        let mut args = [0u8; 52];
        args[0..32].copy_from_slice(rollup_type_hash.as_slice());
        args[32..52].copy_from_slice(&eth_addr);

        let account_script = Script::new_builder()
            .code_hash(eth_account_lock_hash.pack())
            .hash_type(ScriptHashType::Type.into())
            .args(args.pack())
            .build();

        let account_script_hash: H256 = account_script.hash().into();
        let id = state
            .create_account_from_script(account_script.clone())
            .unwrap();
        state
            .mint_sudt(
                CKB_SUDT_ACCOUNT_ID,
                &account_script_hash.as_slice()[0..20],
                BENCH_GENESIS_ACCOUNT_CKB_BALANCE,
            )
            .unwrap();

        Account {
            id,
            sk: *sk.as_ref(),
            eth_addr,
            script: account_script,
        }
    };

    let sks = std::fs::read_to_string(GENESIS_ACCOUNT_SKS).unwrap();
    sks.split('\n').map(build_account).collect()
}
