#![allow(warnings)]

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
mod blockchain {
    include!(concat!(env!("OUT_DIR"), "/blockchain.rs"));
}

#[allow(clippy::all)]
mod godwoken {
    include!(concat!(env!("OUT_DIR"), "/godwoken.rs"));
}

#[cfg(feature = "std")]
#[allow(clippy::all)]
mod store {
    include!(concat!(env!("OUT_DIR"), "/store.rs"));
}

#[cfg(feature = "std")]
#[allow(clippy::all)]
mod mem_block {
    include!(concat!(env!("OUT_DIR"), "/mem_block.rs"));
}

#[cfg(feature = "std")]
#[allow(clippy::all)]
mod omni_lock {
    include!(concat!(env!("OUT_DIR"), "/omni_lock.rs"));
}

#[cfg(feature = "std")]
#[allow(clippy::all)]
mod xudt_rce {
    include!(concat!(env!("OUT_DIR"), "/xudt_rce.rs"));
}

#[cfg(feature = "deprecated")]
#[allow(clippy::all)]
mod deprecated {
    include!(concat!(env!("OUT_DIR"), "/deprecated.rs"));
}

#[cfg(feature = "std")]
#[allow(clippy::all)]
mod exported_block {
    include!(concat!(env!("OUT_DIR"), "/exported_block.rs"));
}

#[cfg(feature = "std")]
#[allow(clippy::all)]
mod block_sync {
    include!(concat!(env!("OUT_DIR"), "/block_sync.rs"));
}

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
