use anyhow::{anyhow, Result};
use gw_common::builtins::CKB_SUDT_ACCOUNT_ID;
use gw_common::state::State;
use gw_common::H256;
use gw_traits::CodeStore;
use gw_types::core::ScriptHashType;
use gw_types::packed::{
    BatchSetMapping, ETHAddrRegArgs, ETHAddrRegArgsUnion, Fee, L2Transaction, RawL2Transaction,
    Script,
};
use gw_types::prelude::{Builder, Entity, Pack};
use gw_utils::wallet::Wallet;
use sha3::{Digest, Keccak256};

pub struct EthEoaMappingRegister {
    rollup_script_hash: H256,
    account_id: u32,
    account_script_hash: H256,
    registry_account_id: u32,
    registry_script_hash: H256,
    eth_lock_code_hash: H256,
    wallet: Wallet,
}

impl EthEoaMappingRegister {
    pub fn create(
        state: &impl State,
        rollup_script_hash: H256,
        eth_registry_code_hash: H256,
        eth_lock_code_hash: H256,
        wallet: Wallet,
    ) -> Result<Self> {
        // NOTE: Use eth_lock_code_hash to ensure register tx can be verified on chain
        let account_script_hash = {
            let script = wallet.eth_lock_script(&rollup_script_hash, &eth_lock_code_hash)?;
            script.hash().into()
        };
        let account_id = state
            .get_account_id_by_script_hash(&account_script_hash)?
            .ok_or_else(|| anyhow!("[eoa mapping] eth register(tx builder) account not found"))?;

        let registry_script_hash = {
            let script = build_registry_script(rollup_script_hash, eth_registry_code_hash);
            script.hash().into()
        };
        let registry_account_id = state
            .get_account_id_by_script_hash(&registry_script_hash)?
            .ok_or_else(|| anyhow!("[eoa mapping] eth registry(contract) account not found"))?;

        let register = EthEoaMappingRegister {
            rollup_script_hash,
            account_id,
            account_script_hash,
            registry_account_id,
            registry_script_hash,
            eth_lock_code_hash,
            wallet,
        };

        Ok(register)
    }

    pub fn registry_account_id(&self) -> u32 {
        self.registry_account_id
    }

    pub fn lock_code_hash(&self) -> &H256 {
        &self.eth_lock_code_hash
    }

    pub fn filter_accounts(
        &self,
        state: &(impl State + CodeStore),
        from_id: u32,
        to_id: u32,
    ) -> Result<Vec<H256>> {
        assert!(from_id <= to_id);

        let eth_lock_code_hash = self.eth_lock_code_hash.pack();
        let mut script_hashes = Vec::with_capacity(to_id.saturating_sub(from_id) as usize + 1);
        for id in from_id..=to_id {
            let script_hash = state.get_script_hash(id)?;
            match state.get_script(&script_hash) {
                Some(script) if script.code_hash() == eth_lock_code_hash => {
                    script_hashes.push(script_hash)
                }
                _ => continue,
            }
        }

        Ok(script_hashes)
    }

    pub fn build_register_tx(
        &self,
        state: &impl State,
        script_hashes: Vec<H256>,
    ) -> Result<L2Transaction> {
        let nonce = state.get_nonce(self.account_id)?;

        let fee = Fee::new_builder()
            .amount(0.pack())
            .sudt_id(CKB_SUDT_ACCOUNT_ID.pack())
            .build();

        let batch_set_mapping = BatchSetMapping::new_builder()
            .fee(fee)
            .gw_script_hashes(script_hashes.pack())
            .build();

        let args = ETHAddrRegArgs::new_builder()
            .set(ETHAddrRegArgsUnion::BatchSetMapping(batch_set_mapping))
            .build();

        let raw_l2tx = RawL2Transaction::new_builder()
            .from_id(self.account_id.pack())
            .to_id(self.registry_account_id.pack())
            .nonce(nonce.pack())
            .args(args.as_bytes().pack())
            .build();

        let message = raw_l2tx.calc_message(
            &self.rollup_script_hash,
            &self.account_script_hash,
            &self.registry_script_hash,
        );
        let signing_message = {
            let mut hasher = Keccak256::new();
            hasher.update("\x19Ethereum Signed Message:\n32");
            hasher.update(message.as_slice());
            let buf = hasher.finalize();
            let mut signing_message = [0u8; 32];
            signing_message.copy_from_slice(&buf[..]);
            signing_message
        };

        let sign = self.wallet.sign_message(signing_message)?;
        let tx = L2Transaction::new_builder()
            .raw(raw_l2tx)
            .signature(sign.pack())
            .build();

        Ok(tx)
    }
}

pub fn build_registry_script(rollup_script_hash: H256, eth_registry_code_hash: H256) -> Script {
    Script::new_builder()
        .code_hash(eth_registry_code_hash.pack())
        .hash_type(ScriptHashType::Type.into())
        .args(rollup_script_hash.as_slice().to_vec().pack())
        .build()
}
