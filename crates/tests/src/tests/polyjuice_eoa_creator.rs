#![allow(clippy::mutable_key_type)]

use anyhow::Result;
use ckb_crypto::secp::Privkey;
use ckb_types::prelude::{Builder, Entity};
use gw_common::{
    builtins::{CKB_SUDT_ACCOUNT_ID, ETH_REGISTRY_ACCOUNT_ID},
    registry::eth_registry::extract_eth_address_from_eoa,
    registry_address::RegistryAddress,
    state::State,
    H256,
};
use gw_generator::{account_lock_manage::secp256k1::Secp256k1Eth, traits::StateExt};
use gw_rpc_server::polyjuice_tx::eoa_creator::PolyjuiceEthEoaCreator;
use gw_types::{
    bytes::Bytes,
    core::{AllowedContractType, AllowedEoaType, ScriptHashType},
    packed::{AllowedTypeHash, L2Transaction, RawL2Transaction, RollupConfig, Script},
    prelude::{Pack, PackVec, Unpack},
    U256,
};
use gw_utils::wallet::{privkey_to_eth_account_script, Wallet};
use secp256k1::{rand::rngs::OsRng, Context, Secp256k1, Signing};

use crate::testing_tool::chain::{
    setup_chain_with_config, ETH_ACCOUNT_LOCK_CODE_HASH, META_VALIDATOR_SCRIPT_TYPE_HASH,
    POLYJUICE_VALIDATOR_CODE_HASH, SUDT_VALIDATOR_CODE_HASH,
};

#[tokio::test(flavor = "multi_thread")]
async fn test_polyjuice_eoa_creator() -> Result<()> {
    let _ = env_logger::builder().is_test(true).try_init();

    let rollup_type_script = Script::default();
    let rollup_script_hash: H256 = rollup_type_script.hash().into();

    let chain_id = 33u64;
    let rollup_config = RollupConfig::new_builder()
        .allowed_eoa_type_hashes(
            vec![AllowedTypeHash::new(
                AllowedEoaType::Eth,
                *ETH_ACCOUNT_LOCK_CODE_HASH,
            )]
            .pack(),
        )
        .allowed_contract_type_hashes(
            vec![AllowedTypeHash::new(
                AllowedContractType::Meta,
                META_VALIDATOR_SCRIPT_TYPE_HASH,
            )]
            .pack(),
        )
        .l2_sudt_validator_script_type_hash(SUDT_VALIDATOR_CODE_HASH.pack())
        .finality_blocks(1u64.pack())
        .chain_id(chain_id.pack())
        .build();

    let chain = setup_chain_with_config(rollup_type_script.clone(), rollup_config).await;
    let mem_pool_state = {
        let mem_pool = chain.mem_pool().as_ref().unwrap();
        mem_pool.lock().await.mem_pool_state()
    };
    let snap = mem_pool_state.load();
    let mut state = snap.state()?;

    let secp = Secp256k1::new();
    let mut rng = OsRng::new().expect("OsRng");

    let creator_wallet = EthWallet::random(rollup_script_hash, &secp, &mut rng);
    state.create_account_from_script(creator_wallet.eoa_account_script())?;
    state.mapping_registry_address_to_script_hash(
        creator_wallet.registry_address.to_owned(),
        creator_wallet.eoa_account_script().hash().into(),
    )?;

    let polyjuice_script = {
        let mut args = rollup_script_hash.as_slice().to_vec();
        args.extend_from_slice(&CKB_SUDT_ACCOUNT_ID.to_le_bytes());
        Script::new_builder()
            .code_hash(POLYJUICE_VALIDATOR_CODE_HASH.pack())
            .hash_type(ScriptHashType::Type.into())
            .args(args.pack())
            .build()
    };
    let polyjuice_account_id = state.create_account_from_script(polyjuice_script.clone())?;

    let eth_eoa_creator = PolyjuiceEthEoaCreator::create(
        &state,
        chain_id,
        rollup_script_hash,
        (*ETH_ACCOUNT_LOCK_CODE_HASH).into(),
        creator_wallet.inner,
    )?;

    let eth_eoa_count = 5;
    let eth_eoa_wallet: Vec<_> = (0..eth_eoa_count)
        .map(|_| EthWallet::random(rollup_script_hash, &secp, &mut rng))
        .collect();

    for wallet in eth_eoa_wallet.iter() {
        state.mint_sudt(CKB_SUDT_ACCOUNT_ID, &wallet.registry_address, U256::one())?;
    }

    let polyjuice_create_args = {
        let mut polyjuice_args = vec![0u8; 69];
        polyjuice_args[0..7].copy_from_slice(b"\xFF\xFF\xFFPOLY");
        polyjuice_args[7] = 3;
        let gas_limit: u64 = 21000;
        polyjuice_args[8..16].copy_from_slice(&gas_limit.to_le_bytes());
        let gas_price: u128 = 20000000000;
        polyjuice_args[16..32].copy_from_slice(&gas_price.to_le_bytes());
        let value: u128 = 3000000;
        polyjuice_args[32..48].copy_from_slice(&value.to_le_bytes());
        let payload_length: u32 = 17;
        polyjuice_args[48..52].copy_from_slice(&payload_length.to_le_bytes());
        polyjuice_args[52..69].copy_from_slice(b"POLYJUICEcontract");
        polyjuice_args
    };

    let raw_tx = RawL2Transaction::new_builder()
        .chain_id(chain_id.pack())
        .from_id(0u32.pack())
        .to_id(polyjuice_account_id.pack())
        .nonce(0u32.pack())
        .args(polyjuice_create_args.pack())
        .build();

    let signing_message =
        Secp256k1Eth::polyjuice_tx_signing_message(chain_id, &raw_tx, &polyjuice_script)?;

    let txs = eth_eoa_wallet
        .iter()
        .map(|wallet| {
            let sign = wallet.sign_message(signing_message.into())?;
            let tx = L2Transaction::new_builder()
                .raw(raw_tx.clone())
                .signature(sign.pack())
                .build();
            Ok(tx)
        })
        .collect::<Result<Vec<_>>>()?;

    let map_sig_eoa_scripts = eth_eoa_creator.filter_map_from_id_zero_has_ckb_balance(&state, &txs);
    assert_eq!(map_sig_eoa_scripts.len(), eth_eoa_wallet.len());

    let batch_create_tx =
        eth_eoa_creator.build_batch_create_tx(&state, map_sig_eoa_scripts.values())?;

    {
        let opt_mem_pool = chain.mem_pool().as_ref();
        let mut mem_pool = opt_mem_pool.unwrap().lock().await;
        mem_pool.push_transaction(batch_create_tx).await?;
    }

    for wallet in eth_eoa_wallet {
        let opt_eoa_account_script_hash =
            state.get_script_hash_by_registry_address(&wallet.registry_address)?;

        assert_eq!(
            opt_eoa_account_script_hash,
            Some(wallet.eoa_account_script().hash().into())
        );
    }

    Ok(())
}

struct EthWallet {
    inner: Wallet,
    registry_address: RegistryAddress,
}

impl EthWallet {
    fn random<C: Context + Signing>(
        rollup_script_hash: H256,
        secp: &Secp256k1<C>,
        rng: &mut OsRng,
    ) -> Self {
        let privkey = {
            let (sk, _public_key) = secp.generate_keypair(rng);
            Privkey::from_slice(&sk.serialize_secret())
        };

        let account_script = privkey_to_eth_account_script(
            &privkey,
            &rollup_script_hash,
            &(*ETH_ACCOUNT_LOCK_CODE_HASH).into(),
        )
        .expect("random wallet");

        let eth_address = {
            let args: Bytes = account_script.args().unpack();
            extract_eth_address_from_eoa(&args).expect("eth address")
        };
        let registry_address = RegistryAddress::new(ETH_REGISTRY_ACCOUNT_ID, eth_address);
        let wallet = Wallet::new(privkey, account_script);

        EthWallet {
            inner: wallet,
            registry_address,
        }
    }

    fn eoa_account_script(&self) -> Script {
        self.inner.lock_script().to_owned()
    }

    fn sign_message(&self, msg: [u8; 32]) -> Result<[u8; 65]> {
        self.inner.sign_message(msg)
    }
}
