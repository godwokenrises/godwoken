use molecule::prelude::Byte;

use crate::packed::{self, GlobalState, GlobalStateV0};
use crate::prelude::{Builder, Entity, Pack};
use core::convert::TryFrom;
use core::convert::TryInto;

// re-export H256
pub use sparse_merkle_tree::H256;

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

impl From<GlobalStateV0> for GlobalState {
    fn from(global_state_v0: GlobalStateV0) -> GlobalState {
        GlobalState::new_builder()
            .rollup_config_hash(global_state_v0.rollup_config_hash())
            .account(global_state_v0.account())
            .block(global_state_v0.block())
            .reverted_block_root(global_state_v0.reverted_block_root())
            .tip_block_hash(global_state_v0.tip_block_hash())
            .last_finalized_timepoint(global_state_v0.last_finalized_timepoint())
            .status(global_state_v0.status())
            .tip_block_timestamp(0u64.pack())
            .version(0.into())
            .build()
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

/// Timepoint encodes in the below layout into u64 in order to support representing two kinds of
/// time points, block number and timestamp.
///   - the highest 1 bit represent whether the time point is block-number-based or timestamp-based
///   - the rest 63 bits represent the value of time point
#[derive(Clone, PartialEq, Eq, Debug)]
pub enum Timepoint {
    BlockNumber(u64),
    Timestamp(u64),
}

impl Default for Timepoint {
    fn default() -> Self {
        Timepoint::BlockNumber(0)
    }
}

impl Timepoint {
    const MASK: u64 = 1 << 63;

    pub const fn from_block_number(block_number: u64) -> Self {
        Timepoint::BlockNumber(block_number)
    }

    pub const fn from_timestamp(timestamp: u64) -> Self {
        Timepoint::Timestamp(timestamp)
    }

    pub const fn from_full_value(full_value: u64) -> Self {
        let is_block_number = (Self::MASK & full_value) == 0;
        if is_block_number {
            Timepoint::BlockNumber(full_value)
        } else {
            Timepoint::Timestamp(Self::MASK ^ full_value)
        }
    }

    pub const fn full_value(&self) -> u64 {
        match self {
            Timepoint::BlockNumber(block_number) => *block_number,
            Timepoint::Timestamp(timestamp) => Self::MASK | *timestamp,
        }
    }
}

mod tests {
    #[test]
    fn test_timepoint_from_block_number() {
        let block_number: u64 = 123;
        let timepoint = super::Timepoint::from_block_number(block_number);
        assert_eq!(timepoint, super::Timepoint::BlockNumber(block_number));
        assert_eq!(timepoint.full_value(), block_number);
        assert_eq!(timepoint, super::Timepoint::from_full_value(block_number))
    }

    #[test]
    fn test_timepoint_from_timestamp() {
        let timestamp: u64 = 1557311768;
        let timepoint = super::Timepoint::from_timestamp(timestamp);
        assert_eq!(timepoint, super::Timepoint::Timestamp(timestamp));
        assert_eq!(timepoint.full_value(), timestamp | (1 << 63));
        assert_eq!(
            timepoint,
            super::Timepoint::from_full_value(timestamp | (1 << 63))
        )
    }
}
