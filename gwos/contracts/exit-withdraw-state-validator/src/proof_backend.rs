//! Thin ABI adapter to upstream sparse-merkle-tree 0.6.1 C implementation; no algorithm changes.
use alloc::vec::Vec;
use gw_smt::sparse_merkle_tree::{error::Error, CompiledMerkleProof, H256};
pub trait ProofBackend {
    fn backend_root(&self, leaves: Vec<(H256, H256)>) -> Result<H256, Error>;
}
impl ProofBackend for CompiledMerkleProof {
    fn backend_root(&self, leaves: Vec<(H256, H256)>) -> Result<H256, Error> {
        #[cfg(not(feature = "onchain"))]
        {
            self.compute_root::<gw_smt::smt::Blake2bHasher>(leaves)
        }
        #[cfg(feature = "onchain")]
        {
            c_root(&self.0, leaves)
        }
    }
}
#[cfg(feature = "onchain")]
#[repr(C)]
struct Pair {
    key: [u8; 32],
    value: [u8; 32],
    order: u32,
}
#[cfg(feature = "onchain")]
#[repr(C)]
struct State {
    pairs: *mut Pair,
    len: u32,
    capacity: u32,
}
#[cfg(feature = "onchain")]
#[link(name = "smt-c-impl", kind = "static")]
extern "C" {
    fn smt_calculate_root(out: *mut u8, state: *const State, proof: *const u8, len: u32) -> i32;
}
#[cfg(feature = "onchain")]
fn c_root(proof: &[u8], mut leaves: Vec<(H256, H256)>) -> Result<H256, Error> {
    if leaves.len() > u32::MAX as usize || proof.len() > u32::MAX as usize {
        return Err(Error::CorruptedProof);
    }
    // Upstream root API requires sorted, unique leaves. Reject duplicates instead of normalizing away witness entries.
    leaves.sort_unstable_by_key(|(k, _)| *k);
    if leaves.windows(2).any(|w| w[0].0 == w[1].0) {
        return Err(Error::CorruptedProof);
    }
    let mut buffer: Vec<Pair> = leaves
        .into_iter()
        .map(|(k, v)| Pair {
            key: k.into(),
            value: v.into(),
            order: 0,
        })
        .collect();
    let state = State {
        pairs: buffer.as_mut_ptr(),
        len: buffer.len() as u32,
        capacity: buffer.len() as u32,
    };
    let mut out = [0u8; 32];
    unsafe {
        if smt_calculate_root(out.as_mut_ptr(), &state, proof.as_ptr(), proof.len() as u32) != 0 {
            return Err(Error::CorruptedProof);
        }
    }
    Ok(out.into())
}
