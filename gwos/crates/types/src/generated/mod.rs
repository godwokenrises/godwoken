#![allow(warnings)]
#![allow(unused_imports)]

// Re-export ckb-types instead of using types in blockchain.rs.
#[cfg(feature = "std")]
mod blockchain {
    pub use ckb_types::packed::{
        BeUint32, BeUint32Reader, Block, BlockReader, Byte32, Byte32Reader, Byte32Vec,
        Byte32VecReader, Bytes, BytesOpt, BytesOptReader, BytesReader, BytesVec, BytesVecReader,
        CellDep, CellDepReader, CellDepVec, CellDepVecReader, CellInput, CellInputReader,
        CellInputVec, CellInputVecReader, CellOutput, CellOutputReader, CellOutputVec,
        CellOutputVecReader, NumberHash, NumberHashReader, OutPoint, OutPointReader,
        ProposalShortId, ProposalShortIdReader, RawTransaction, RawTransactionReader, Script,
        ScriptOpt, ScriptOptReader, ScriptReader, Transaction, TransactionKey,
        TransactionKeyReader, TransactionReader, Uint128, Uint128Reader, Uint16, Uint16Reader,
        Uint256, Uint256Reader, Uint32, Uint32Reader, Uint32Vec, Uint32VecReader, Uint64,
        Uint64Reader, WitnessArgs, WitnessArgsReader,
    };
}

// Use generated types.
#[cfg(not(feature = "std"))]
mod blockchain;

#[allow(clippy::all)]
mod godwoken;

#[cfg(feature = "std")]
#[allow(clippy::all)]
mod store;

#[cfg(feature = "std")]
#[allow(clippy::all)]
mod mem_block;

#[cfg(feature = "std")]
#[allow(clippy::all)]
mod omni_lock;

#[cfg(feature = "std")]
#[allow(clippy::all)]
mod xudt_rce;

#[cfg(feature = "deprecated")]
#[allow(clippy::all)]
mod deprecated;

#[cfg(feature = "std")]
#[allow(clippy::all)]
mod exported_block;

#[cfg(feature = "std")]
#[allow(clippy::all)]
mod block_sync;

pub mod packed {
    pub use molecule::prelude::*;

    #[cfg(feature = "std")]
    pub use super::block_sync::*;
    #[cfg(feature = "deprecated")]
    pub use super::deprecated::*;
    #[cfg(feature = "std")]
    pub use super::exported_block::*;
    #[cfg(feature = "std")]
    pub use super::mem_block::*;
    #[cfg(feature = "std")]
    pub use super::omni_lock::*;
    #[cfg(feature = "std")]
    pub use super::store::*;
    pub use super::{blockchain::*, godwoken::*};
}
