use molecule::prelude::Byte;

use crate::packed;
use core::convert::TryFrom;
use core::convert::TryInto;

#[derive(Clone, Copy, PartialEq, Eq)]
pub enum ScriptHashType {
    Data = 0,
    Type = 1,
}

impl Into<packed::Byte> for ScriptHashType {
    fn into(self) -> packed::Byte {
        (self as u8).into()
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

impl Into<u8> for Status {
    fn into(self: Status) -> u8 {
        self as u8
    }
}

impl TryFrom<u8> for Status {
    type Error = u8;
    fn try_from(value: u8) -> Result<Self, Self::Error> {
        match value {
            0 => Ok(Status::Running),
            1 => Ok(Status::Halting),
            n => return Err(n),
        }
    }
}

impl Into<Byte> for Status {
    fn into(self: Status) -> Byte {
        (self as u8).into()
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
    Transaction = 0,
    Withdrawal = 1,
}

impl Into<u8> for ChallengeTargetType {
    fn into(self: ChallengeTargetType) -> u8 {
        self as u8
    }
}

impl TryFrom<u8> for ChallengeTargetType {
    type Error = u8;
    fn try_from(value: u8) -> Result<Self, Self::Error> {
        match value {
            0 => Ok(ChallengeTargetType::Transaction),
            1 => Ok(ChallengeTargetType::Withdrawal),
            n => return Err(n),
        }
    }
}

impl Into<Byte> for ChallengeTargetType {
    fn into(self: ChallengeTargetType) -> Byte {
        (self as u8).into()
    }
}

impl TryFrom<Byte> for ChallengeTargetType {
    type Error = u8;
    fn try_from(value: Byte) -> Result<Self, Self::Error> {
        let v: u8 = value.into();
        v.try_into()
    }
}
