use anyhow::{anyhow, bail, Result};
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
use gw_traits::CodeStore;
use gw_types::{
    bytes::Bytes,
    packed::{L2Transaction, RawL2Transaction, Script},
    prelude::{Pack, Unpack},
    U256,
};
use gw_utils::wallet::{privkey_to_eth_account_script, Wallet};
use rand::{rngs::OsRng, Rng};
use secp256k1::SecretKey;

use crate::testing_tool::chain::ETH_ACCOUNT_LOCK_CODE_HASH;

pub struct EthWallet {
    pub inner: Wallet,
    pub registry_address: RegistryAddress,
}

impl EthWallet {
    pub fn random(rollup_script_hash: H256) -> Self {
        let privkey = {
            let sk = SecretKey::from_slice(&OsRng.gen::<[u8; 32]>()).expect("generating SecretKey");
            Privkey::from_slice(&sk.secret_bytes())
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
        assert_eq!(eth_address.len(), 20);
        let registry_address = RegistryAddress::new(ETH_REGISTRY_ACCOUNT_ID, eth_address);
        let wallet = Wallet::new(privkey, account_script);

        EthWallet {
            inner: wallet,
            registry_address,
        }
    }

    pub fn reg_address(&self) -> &RegistryAddress {
        &self.registry_address
    }

    pub fn account_script(&self) -> &Script {
        self.inner.lock_script()
    }

    pub fn account_script_hash(&self) -> H256 {
        self.inner.lock_script().hash().into()
    }

    pub fn sign_polyjuice_tx(
        &self,
        state: &(impl State + CodeStore),
        raw_tx: RawL2Transaction,
    ) -> Result<L2Transaction> {
        let to_id: u32 = raw_tx.to_id().unpack();

        let to_script_hash = state.get_script_hash(to_id)?;
        if to_script_hash.is_zero() {
            bail!("to id {} script hash not found", to_id);
        }
        let to_script = state
            .get_script(&to_script_hash)
            .ok_or_else(|| anyhow!("to id {} script not found", to_id))?;

        let chain_id = raw_tx.chain_id().unpack();
        let signing_message =
            Secp256k1Eth::polyjuice_tx_signing_message(chain_id, &raw_tx, &to_script)?;
        let sig = self.sign_message(signing_message.into())?;

        let tx = L2Transaction::new_builder()
            .raw(raw_tx)
            .signature(sig.pack())
            .build();

        Ok(tx)
    }

    pub fn sign_message(&self, msg: [u8; 32]) -> Result<[u8; 65]> {
        self.inner.sign_message(msg)
    }

    pub fn create_account(
        &self,
        state: &mut (impl State + StateExt),
        ckb_balance: U256,
    ) -> Result<u32> {
        let account_id = state.create_account_from_script(self.account_script().to_owned())?;

        state.mapping_registry_address_to_script_hash(
            self.registry_address.to_owned(),
            self.account_script().hash().into(),
        )?;
        state.mint_sudt(CKB_SUDT_ACCOUNT_ID, &self.registry_address, ckb_balance)?;

        Ok(account_id)
    }

    pub fn mint_ckb_sudt(&self, state: &mut impl State, amount: U256) -> Result<()> {
        state.mint_sudt(CKB_SUDT_ACCOUNT_ID, &self.registry_address, amount)?;
        Ok(())
    }

    pub fn mint_sudt(&self, state: &mut impl State, sudt_id: u32, amount: U256) -> Result<()> {
        state.mint_sudt(sudt_id, &self.registry_address, amount)?;
        Ok(())
    }
}
