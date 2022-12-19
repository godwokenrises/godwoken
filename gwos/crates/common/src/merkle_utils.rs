use merkle_cbt::{merkle_tree::Merge, MerkleProof as ExMerkleProof, CBMT as ExCBMT};

use crate::blake2b::new_blake2b;
use crate::vec::Vec;
use gw_types::h256::H256;

// Calculate compacted account root
pub fn calculate_state_checkpoint(root: &H256, count: u32) -> H256 {
    let mut hash = [0u8; 32];
    let mut hasher = new_blake2b();
    hasher.update(root.as_slice());
    hasher.update(&count.to_le_bytes());
    hasher.finalize(&mut hash);
    hash
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
        hash
    }
}

pub type CBMT = ExCBMT<H256, MergeH256>;
pub type CBMTMerkleProof = ExMerkleProof<H256, MergeH256>;

pub fn calculate_ckb_merkle_root(leaves: Vec<H256>) -> H256 {
    let tree = CBMT::build_merkle_tree(&leaves);
    tree.root()
}

/// blake2b(index(u32) | item_hash)
pub fn ckb_merkle_leaf_hash(index: u32, item_hash: &H256) -> H256 {
    let mut hasher = new_blake2b();
    hasher.update(&index.to_le_bytes());
    hasher.update(item_hash.as_slice());
    let mut buf = [0u8; 32];
    hasher.finalize(&mut buf);
    buf
}

#[cfg(test)]
mod tests {
    use gw_types::h256::H256;

    #[test]
    fn merkle_proof_test() {
        let leaves: Vec<H256> = vec![[0u8; 32], [1u8; 32], [2u8; 32], [3u8; 32], [4u8; 32]];
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
            proof.indices().to_vec(),
            proof.lemmas().to_vec(),
        );

        let proof_leaves: Vec<H256> = vec![[0u8; 32], [4u8; 32]];
        let proof_leaves = indices
            .into_iter()
            .zip(proof_leaves)
            .map(|(i, hash)| crate::merkle_utils::ckb_merkle_leaf_hash(i, &hash))
            .collect::<Vec<_>>();

        assert!(proof.verify(&root, &proof_leaves));

        let proof_leaves = vec![[1u8; 32], [3u8; 32]];
        assert!(!proof.verify(&root, &proof_leaves));
    }
}
