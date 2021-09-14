use merkle_cbt::{merkle_tree::Merge, MerkleProof as ExMerkleProof, CBMT as ExCBMT};

use crate::vec::Vec;
use crate::{
    blake2b::new_blake2b,
    h256_ext::H256Ext,
    smt::{default_store::DefaultStore, Error, H256, SMT},
};

// Calculate compacted account root
pub fn calculate_state_checkpoint(root: &H256, count: u32) -> H256 {
    let mut hash = [0u8; 32];
    let mut hasher = new_blake2b();
    hasher.update(root.as_slice());
    hasher.update(&count.to_le_bytes());
    hasher.finalize(&mut hash);
    hash.into()
}

/// Compute merkle root from vectors
pub fn calculate_merkle_root(leaves: Vec<H256>) -> Result<H256, Error> {
    if leaves.is_empty() {
        return Ok(H256::zero());
    }
    let mut tree = SMT::<DefaultStore<H256>>::default();
    for (i, leaf) in leaves.into_iter().enumerate() {
        tree.update(H256::from_u32(i as u32), leaf)?;
    }
    Ok(*tree.root())
}

pub struct MergeH256;

impl Merge for MergeH256 {
    type Item = H256;
    fn merge(left: &Self::Item, right: &Self::Item) -> Self::Item {
        let mut hash = [0u8; 32];
        let mut blake2b = new_blake2b();

        blake2b.update(left.as_slice());
        blake2b.update(right.as_slice());
        blake2b.finalize(&mut hash);
        hash.into()
    }
}

pub type CBMT = ExCBMT<H256, MergeH256>;
pub type CBMTMerkleProof = ExMerkleProof<H256, MergeH256>;

pub fn calculate_ckb_merkle_root(leaves: Vec<H256>) -> Result<H256, Error> {
    let tree = CBMT::build_merkle_tree(&leaves);
    Ok(tree.root())
}

/// blake2b(index(u32) | item_hash)
pub fn ckb_merkle_leaf_hash(index: u32, item_hash: &H256) -> H256 {
    let mut hasher = new_blake2b();
    hasher.update(&index.to_le_bytes());
    hasher.update(item_hash.as_slice());
    let mut buf = [0u8; 32];
    hasher.finalize(&mut buf);
    buf.into()
}

mod tests {

    #[test]
    fn merkle_proof_test() {
        let leaves: Vec<crate::smt::H256> = vec![
            [0u8; 32].into(),
            [1u8; 32].into(),
            [2u8; 32].into(),
            [3u8; 32].into(),
            [4u8; 32].into(),
        ];
        let leaves = leaves
            .into_iter()
            .enumerate()
            .map(|(index, hash)| crate::merkle_utils::ckb_merkle_leaf_hash(index as u32, &hash))
            .collect::<Vec<_>>();
        let root = crate::merkle_utils::CBMT::build_merkle_root(&leaves);

        let indices = vec![0, 4];
        let proof = crate::merkle_utils::CBMT::build_merkle_proof(&leaves, &indices);
        assert!(proof.is_some());
        //generate proof
        let proof = proof.unwrap();

        //rebuild proof
        let proof = crate::merkle_utils::CBMTMerkleProof::new(
            proof.indices().iter().copied().collect(),
            proof.lemmas().iter().copied().collect(),
        );

        let proof_leaves: Vec<crate::smt::H256> = vec![[0u8; 32].into(), [4u8; 32].into()];
        let proof_leaves = indices
            .into_iter()
            .zip(proof_leaves)
            .map(|(i, hash)| crate::merkle_utils::ckb_merkle_leaf_hash(i, &hash))
            .collect::<Vec<_>>();

        assert!(proof.verify(&root, &proof_leaves));

        let proof_leaves = vec![[1u8; 32].into(), [3u8; 32].into()];
        assert!(!proof.verify(&root, &proof_leaves));
    }
}
