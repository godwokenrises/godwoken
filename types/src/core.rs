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

#[derive(Clone, Copy, PartialEq, Eq)]
pub enum CallType {
    Construct = 0,
    HandleMessage = 1,
}

impl Into<u8> for CallType {
    fn into(self) -> u8 {
        self as u8
    }
}

impl Into<Byte> for CallType {
    fn into(self) -> Byte {
        (self as u8).into()
    }
}
