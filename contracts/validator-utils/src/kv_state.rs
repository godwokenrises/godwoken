use alloc::collections::BTreeMap;
use gw_common::{error::Error, smt::Blake2bHasher, smt::CompiledMerkleProof, state::State, H256};
use gw_types::{bytes::Bytes, packed::KVPairVec, prelude::*};

pub struct KVState {
    kv: BTreeMap<H256, H256>,
    proof: Bytes,
    account_count: u32,
}

impl KVState {
    pub fn new(kv_pairs: KVPairVec, proof: Bytes, account_count: u32) -> Self {
        KVState {
            kv: kv_pairs
                .into_iter()
                .map(|kv_pair| {
                    let (k, v): ([u8; 32], [u8; 32]) = kv_pair.unpack();
                    (k.into(), v.into())
                })
                .collect(),
            proof,
            account_count,
        }
    }
}

impl State for KVState {
    fn get_raw(&self, key: &H256) -> Result<H256, Error> {
        // make sure the key must exists in the kv
        Ok(self.kv.get(key).ok_or(Error::MissingKey)?.clone())
    }
    fn update_raw(&mut self, key: H256, value: H256) -> Result<(), Error> {
        // make sure the key must exists in the kv
        let v = self.kv.get_mut(&key).ok_or(Error::MissingKey)?;
        *v = value;
        Ok(())
    }
    fn get_account_count(&self) -> Result<u32, Error> {
        Ok(self.account_count)
    }
    fn set_account_count(&mut self, count: u32) -> Result<(), Error> {
        self.account_count = count;
        Ok(())
    }
    fn calculate_root(&self) -> Result<H256, Error> {
        let proof = CompiledMerkleProof(self.proof.clone().into());
        let root = proof.compute_root::<Blake2bHasher>(self.kv.clone().into_iter().collect())?;
        Ok(root)
    }
}
