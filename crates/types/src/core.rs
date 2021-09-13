use molecule::prelude::Byte;

use crate::packed;
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
pub enum SignatureMessageType {
    Raw,
    Signing,
}

impl From<SignatureMessageType> for u8 {
    #[inline]
    fn from(type_: SignatureMessageType) -> u8 {
        type_ as u8
    }
}

impl TryFrom<u8> for SignatureMessageType {
    type Error = u8;
    fn try_from(value: u8) -> Result<Self, Self::Error> {
        match value {
            0 => Ok(SignatureMessageType::Raw),
            1 => Ok(SignatureMessageType::Signing),
            n => Err(n),
        }
    }
}
