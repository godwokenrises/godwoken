use std::convert::TryFrom;

use molecule::prelude::Byte;

#[derive(Clone, Copy, PartialEq, Eq)]
pub enum ScriptHashType {
    Data = 0,
    Type = 1,
}

impl Into<u8> for ScriptHashType {
    fn into(self) -> u8 {
        self as u8
    }
}

impl Into<Byte> for ScriptHashType {
    fn into(self) -> Byte {
        (self as u8).into()
    }
}

/// Rollup status
#[derive(Debug, Eq, PartialEq, Copy, Clone)]
#[repr(u8)]
pub enum Status {
    Running = 0,
    Halting = 1,
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
