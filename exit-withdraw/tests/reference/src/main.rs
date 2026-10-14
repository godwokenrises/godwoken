// Independent upstream reference vectors; use the Godwoken hash personalization.
use sparse_merkle_tree::{SparseMerkleTree, default_store::DefaultStore, traits::Hasher, H256};
use blake2b_ref::{Blake2b, Blake2bBuilder};
struct CkbHasher(Blake2b);
impl Default for CkbHasher {
    fn default() -> Self { Self(Blake2bBuilder::new(32).personal(b"ckb-default-hash").build()) }
}
impl Hasher for CkbHasher {
    fn write_h256(&mut self, h: &H256) { self.0.update(h.as_slice()); }
    fn write_byte(&mut self, b: u8) { self.0.update(&[b]); }
    fn finish(self) -> H256 { let mut x = [0u8;32]; self.0.finalize(&mut x); x.into() }
}
fn main() {
    let mut tree: SparseMerkleTree<CkbHasher,H256,DefaultStore<H256>> = SparseMerkleTree::default();
    let cases = [(0u8,1u8),(1,2),(128,3),(255,4),(42,5),(1,0)];
    for (k,v) in cases {
        tree.update([k;32].into(),[v;32].into()).unwrap();
        println!("{} {} {}", k, v, tree.root().as_slice().iter().map(|b|format!("{:02x}",b)).collect::<String>());
    }
}
