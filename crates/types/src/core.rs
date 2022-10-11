use molecule::prelude::Byte;

use crate::packed::{self, GlobalState, GlobalStateV1, L2Block};
use crate::prelude::{Builder, Entity, Pack, Unpack};
use core::convert::TryFrom;
use core::convert::TryInto;

// re-export H256
pub use sparse_merkle_tree::H256;

impl L2Block {
    pub fn number(&self) -> u64 {
        self.raw().number().unpack()
    }
}

#[derive(Clone, Copy, PartialEq, Eq)]
pub enum ScriptHashType {
    Data = 0,
    Type = 1,
}

impl From<ScriptHashType> for packed::Byte {
    #[inline]
    fn from(type_: ScriptHashType) -> Self {
        (type_ as u8).into()
    }
}

impl TryFrom<packed::Byte> for ScriptHashType {
    type Error = u8;

    fn try_from(v: packed::Byte) -> Result<Self, Self::Error> {
        match Into::<u8>::into(v) {
            0 => Ok(ScriptHashType::Data),
            1 => Ok(ScriptHashType::Type),
            n => Err(n),
        }
    }
}

/// Rollup status
#[derive(Debug, Eq, PartialEq, Copy, Clone)]
#[repr(u8)]
pub enum Status {
    Running = 0,
    Halting = 1,
}

impl From<Status> for u8 {
    #[inline]
    fn from(s: Status) -> u8 {
        s as u8
    }
}

impl TryFrom<u8> for Status {
    type Error = u8;
    fn try_from(value: u8) -> Result<Self, Self::Error> {
        match value {
            0 => Ok(Status::Running),
            1 => Ok(Status::Halting),
            n => Err(n),
        }
    }
}

impl From<Status> for Byte {
    #[inline]
    fn from(s: Status) -> Byte {
        (s as u8).into()
    }
}

impl TryFrom<Byte> for Status {
    type Error = u8;
    fn try_from(value: Byte) -> Result<Self, Self::Error> {
        let v: u8 = value.into();
        v.try_into()
    }
}

/// Challenge target type
#[derive(Debug, Eq, PartialEq, Copy, Clone)]
#[repr(u8)]
pub enum ChallengeTargetType {
    TxExecution = 0,
    TxSignature = 1,
    Withdrawal = 2,
}

impl From<ChallengeTargetType> for u8 {
    #[inline]
    fn from(type_: ChallengeTargetType) -> u8 {
        type_ as u8
    }
}

impl TryFrom<u8> for ChallengeTargetType {
    type Error = u8;
    fn try_from(value: u8) -> Result<Self, Self::Error> {
        match value {
            0 => Ok(ChallengeTargetType::TxExecution),
            1 => Ok(ChallengeTargetType::TxSignature),
            2 => Ok(ChallengeTargetType::Withdrawal),
            n => Err(n),
        }
    }
}

impl From<ChallengeTargetType> for Byte {
    #[inline]
    fn from(type_: ChallengeTargetType) -> Byte {
        (type_ as u8).into()
    }
}

impl TryFrom<Byte> for ChallengeTargetType {
    type Error = u8;
    fn try_from(value: Byte) -> Result<Self, Self::Error> {
        let v: u8 = value.into();
        v.try_into()
    }
}

#[derive(Clone, Copy, PartialEq, Eq, Hash)]
pub enum DepType {
    Code = 0,
    DepGroup = 1,
}

impl Default for DepType {
    fn default() -> Self {
        DepType::Code
    }
}

impl TryFrom<packed::Byte> for DepType {
    type Error = u8;

    fn try_from(v: packed::Byte) -> Result<Self, Self::Error> {
        match Into::<u8>::into(v) {
            0 => Ok(DepType::Code),
            1 => Ok(DepType::DepGroup),
            n => Err(n),
        }
    }
}

impl From<DepType> for u8 {
    #[inline]
    fn from(type_: DepType) -> u8 {
        type_ as u8
    }
}

impl From<DepType> for packed::Byte {
    #[inline]
    fn from(type_: DepType) -> packed::Byte {
        (type_ as u8).into()
    }
}

#[derive(Debug, Eq, PartialEq, Copy, Clone)]
#[repr(u8)]
pub enum SigningType {
    Raw,
    WithPrefix,
}

impl From<SigningType> for u8 {
    #[inline]
    fn from(type_: SigningType) -> u8 {
        type_ as u8
    }
}

impl TryFrom<u8> for SigningType {
    type Error = u8;
    fn try_from(value: u8) -> Result<Self, Self::Error> {
        match value {
            0 => Ok(SigningType::Raw),
            1 => Ok(SigningType::WithPrefix),
            n => Err(n),
        }
    }
}

impl GlobalState {
    pub fn version_u8(&self) -> u8 {
        self.version().into()
    }
}

impl From<GlobalStateV1> for GlobalState {
    fn from(global_state_v1: GlobalStateV1) -> GlobalState {
        GlobalState::new_builder()
            .rollup_config_hash(global_state_v1.rollup_config_hash())
            .account(global_state_v1.account())
            .block(global_state_v1.block())
            .reverted_block_root(global_state_v1.reverted_block_root())
            .tip_block_hash(global_state_v1.tip_block_hash())
            .tip_block_timestamp(global_state_v1.tip_block_timestamp())
            .last_finalized_block_number(global_state_v1.last_finalized_block_number())
            .finalized_withdrawal_cursor(packed::WithdrawalCursor::default())
            .status(global_state_v1.status())
            .version(1.into())
            .build()
    }
}

#[derive(Debug, PartialEq, Eq, Clone, Copy)]
pub enum WithdrawalCursorIndex {
    All,
    Index(u32),
}

impl WithdrawalCursorIndex {
    /// build cursor
    pub fn build_cursor_index(index: u32, withdrawal_count: u32) -> Option<Self> {
        if withdrawal_count == 0 && index == 0 || index + 1 == withdrawal_count {
            return Some(Self::All);
        }
        if index >= withdrawal_count {
            // invalid index
            return None;
        }
        Some(Self::Index(index))
    }

    pub fn to_cursor_index(&self) -> u32 {
        match self {
            Self::All => packed::WithdrawalCursor::ALL_WITHDRAWALS,
            Self::Index(index) => *index,
        }
    }
}

#[derive(Debug, PartialEq, Eq, Clone, Copy)]
pub struct WithdrawalCursor {
    pub block_number: u64,
    pub index: WithdrawalCursorIndex,
}

impl WithdrawalCursor {
    /// build cursor
    pub fn build_cursor(block_number: u64, index: u32, withdrawal_count: u32) -> Option<Self> {
        let index = WithdrawalCursorIndex::build_cursor_index(index, withdrawal_count)?;
        Some(Self {
            block_number,
            index,
        })
    }

    pub fn pack_cursor(&self) -> packed::WithdrawalCursor {
        packed::WithdrawalCursor::new_builder()
            .block_number(self.block_number.pack())
            .index(self.index.to_cursor_index().pack())
            .build()
    }
}

impl packed::WithdrawalCursor {
    pub const ALL_WITHDRAWALS: u32 = u32::MAX;

    pub fn unpack_cursor(&self) -> WithdrawalCursor {
        let index: u32 = self.index().unpack();
        let index = if Self::ALL_WITHDRAWALS == index {
            WithdrawalCursorIndex::All
        } else {
            WithdrawalCursorIndex::Index(index)
        };

        WithdrawalCursor {
            block_number: self.block_number().unpack(),
            index,
        }
    }
}

#[derive(Debug, Eq, PartialEq, Clone, Copy)]
#[repr(u8)]
pub enum AllowedEoaType {
    Unknown,
    Eth,
}

impl From<AllowedEoaType> for u8 {
    #[inline]
    fn from(type_: AllowedEoaType) -> u8 {
        type_ as u8
    }
}

impl TryFrom<u8> for AllowedEoaType {
    type Error = u8;

    fn try_from(value: u8) -> Result<Self, Self::Error> {
        match value {
            0 => Ok(AllowedEoaType::Unknown),
            1 => Ok(AllowedEoaType::Eth),
            n => Err(n),
        }
    }
}

impl From<AllowedEoaType> for packed::Byte {
    #[inline]
    fn from(type_: AllowedEoaType) -> packed::Byte {
        (type_ as u8).into()
    }
}

#[derive(Debug, Eq, PartialEq, Clone, Copy)]
#[repr(u8)]
pub enum AllowedContractType {
    Unknown,
    Meta,
    Sudt,
    Polyjuice,
    EthAddrReg,
}

impl From<AllowedContractType> for u8 {
    #[inline]
    fn from(type_: AllowedContractType) -> u8 {
        type_ as u8
    }
}

impl TryFrom<u8> for AllowedContractType {
    type Error = u8;

    fn try_from(value: u8) -> Result<Self, Self::Error> {
        match value {
            0 => Ok(AllowedContractType::Unknown),
            1 => Ok(AllowedContractType::Meta),
            2 => Ok(AllowedContractType::Sudt),
            3 => Ok(AllowedContractType::Polyjuice),
            4 => Ok(AllowedContractType::EthAddrReg),
            n => Err(n),
        }
    }
}

impl From<AllowedContractType> for packed::Byte {
    #[inline]
    fn from(type_: AllowedContractType) -> packed::Byte {
        (type_ as u8).into()
    }
}

impl packed::AllowedTypeHash {
    pub fn new(type_: impl Into<packed::Byte>, hash: impl Pack<packed::Byte32>) -> Self {
        packed::AllowedTypeHash::new_builder()
            .type_(type_.into())
            .hash(hash.pack())
            .build()
    }

    pub fn from_unknown(hash: impl Pack<packed::Byte32>) -> Self {
        packed::AllowedTypeHash::new_builder()
            .type_(packed::Byte::new(0))
            .hash(hash.pack())
            .build()
    }
}
