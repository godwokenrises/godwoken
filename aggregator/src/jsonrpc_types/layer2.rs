use crate::jsonrpc_types::fixed_bytes::Byte65;
use ckb_jsonrpc_types::{JsonBytes, Uint32};
use ckb_types::{bytes::Bytes, H256};
use gw_types::{packed, prelude::*};
use serde::{Deserialize, Serialize};

#[derive(Clone, Serialize, Deserialize, PartialEq, Eq, Hash, Debug, Default)]
#[serde(rename_all = "snake_case")]
pub struct RawL2Transaction {
    pub from_id: Uint32,
    pub to_id: Uint32,
    pub nonce: Uint32,
    pub args: JsonBytes,
}

impl From<RawL2Transaction> for packed::RawL2Transaction {
    fn from(tx: RawL2Transaction) -> Self {
        let RawL2Transaction {
            from_id,
            to_id,
            nonce,
            args,
        } = tx;
        let from_id: u32 = from_id.into();
        let to_id: u32 = to_id.into();
        let nonce: u32 = nonce.into();
        let args: Bytes = args.into_bytes();
        packed::RawL2Transaction::new_builder()
            .from_id(from_id.pack())
            .to_id(to_id.pack())
            .nonce(nonce.pack())
            .args(args.pack())
            .build()
    }
}

#[derive(Clone, Serialize, Deserialize, PartialEq, Eq, Hash, Debug, Default)]
#[serde(rename_all = "snake_case")]
pub struct L2Transaction {
    pub raw: RawL2Transaction,
    pub signature: Byte65,
}

impl From<L2Transaction> for packed::L2Transaction {
    fn from(tx: L2Transaction) -> Self {
        let L2Transaction { raw, signature } = tx;

        packed::L2Transaction::new_builder()
            .raw(raw.into())
            .signature(signature.into())
            .build()
    }
}

#[derive(Clone, Default, Serialize, Deserialize, PartialEq, Eq, Hash, Debug)]
#[serde(rename_all = "snake_case")]
pub struct L2TransactionView {
    #[serde(flatten)]
    pub inner: L2Transaction,
    pub hash: H256,
}
