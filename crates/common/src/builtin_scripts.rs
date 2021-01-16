use crate::code_hash;
use crate::H256;
use gw_types::bytes::Bytes;
use lazy_static::lazy_static;

lazy_static! {
    pub static ref SUDT_GENERATOR: Bytes = include_bytes!("../../../c/build/sudt-generator")
        .to_vec()
        .into();
    pub static ref SUDT_VALIDATOR: Bytes = include_bytes!("../../../c/build/sudt-validator")
        .to_vec()
        .into();
    pub static ref SUDT_VALIDATOR_CODE_HASH: H256 = code_hash(&SUDT_VALIDATOR);
    pub static ref META_CONTRACT_GENERATOR: Bytes =
        include_bytes!("../../../c/build/meta-contract-generator")
            .to_vec()
            .into();
    pub static ref META_CONTRACT_VALIDATOR: Bytes =
        include_bytes!("../../../c/build/meta-contract-validator")
            .to_vec()
            .into();
    pub static ref META_CONTRACT_VALIDATOR_CODE_HASH: H256 = code_hash(&META_CONTRACT_VALIDATOR);
    pub static ref ETH_ACCOUNT_LOCK: Bytes =
        include_bytes!("../../../c/build/account_locks/eth-account-lock")
            .to_vec()
            .into();
}
