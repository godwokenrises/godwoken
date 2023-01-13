pub extern crate ckb_jsonrpc_types;

// Make migration easier.
pub use ckb_jsonrpc_types as blockchain;

pub mod debug;
pub mod debugger;
pub mod godwoken;
pub mod test_mode;

pub mod number_hash {
    use ckb_jsonrpc_types::BlockNumber;
    use gw_types::{packed, prelude::*};
    use serde::{Deserialize, Serialize};

    #[derive(Serialize, Deserialize, Debug, PartialEq, Eq, Clone)]
    pub struct NumberHash {
        pub block_hash: ckb_fixed_hash::H256,
        pub block_number: BlockNumber,
    }

    impl From<packed::NumberHash> for NumberHash {
        fn from(p: packed::NumberHash) -> Self {
            Self {
                block_hash: p.block_hash().unpack(),
                block_number: p.number().unpack().into(),
            }
        }
    }
    impl From<NumberHash> for packed::NumberHash {
        fn from(nh: NumberHash) -> Self {
            (&nh).into()
        }
    }
    impl From<&NumberHash> for packed::NumberHash {
        fn from(nh: &NumberHash) -> Self {
            Self::new_builder()
                .block_hash(nh.block_hash.pack())
                .number(u64::from(nh.block_number).pack())
                .build()
        }
    }
}

pub trait JsonCalcHash {
    fn hash(&self) -> ckb_fixed_hash::H256;
}

mod impls {
    use ckb_fixed_hash::H256;
    use ckb_jsonrpc_types::Script;
    use gw_types::{packed, prelude::*};

    impl super::JsonCalcHash for Script {
        fn hash(&self) -> H256 {
            packed::Script::from(self.clone())
                .calc_script_hash()
                .unpack()
        }
    }
}
