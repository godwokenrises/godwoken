use sparse_merkle_tree::H256;

#[derive(Clone)]
pub struct BackendInfo {
    pub validator_code_hash: H256,
    pub generator_code_hash: H256,
    pub validator_script_type_hash: H256,
}
