use faster_hex::{hex_decode, hex_encode};
use gw_types::{packed, prelude::*};
use std::fmt::{self, Debug};
use std::hash::{Hash, Hasher};

#[derive(Clone)]
pub struct Byte65(pub [u8; 65]);

impl Default for Byte65 {
    fn default() -> Self {
        Byte65([0u8; 65])
    }
}

impl PartialEq for Byte65 {
    fn eq(&self, other: &Byte65) -> bool {
        self.0[..] == other.0[..]
    }
}

impl Eq for Byte65 {}

impl Hash for Byte65 {
    fn hash<H: Hasher>(&self, state: &mut H) {
        state.write(&self.0);
    }
}

impl Debug for Byte65 {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        self.0.fmt(f)
    }
}

impl Byte65 {
    pub fn new(inner: [u8; 65]) -> Self {
        Byte65(inner)
    }
}

impl From<packed::Signature> for Byte65 {
    fn from(packed: packed::Signature) -> Self {
        let mut inner: [u8; 65] = [0u8; 65];
        inner.copy_from_slice(&packed.raw_data());
        Byte65(inner)
    }
}

impl From<Byte65> for packed::Signature {
    fn from(json: Byte65) -> Self {
        Self::from_slice(&json.0).expect("impossible: fail to read inner array")
    }
}

struct Byte65Visitor;

impl<'b> serde::de::Visitor<'b> for Byte65Visitor {
    type Value = Byte65;

    fn expecting(&self, formatter: &mut fmt::Formatter) -> fmt::Result {
        write!(formatter, "a 0x-prefixed hex string")
    }

    fn visit_str<E>(self, v: &str) -> Result<Self::Value, E>
    where
        E: serde::de::Error,
    {
        if v.len() < 2 || &v.as_bytes()[0..2] != b"0x" || v.len() != 132 {
            return Err(E::invalid_value(serde::de::Unexpected::Str(v), &self));
        }
        let mut buffer = [0u8; 65]; // we checked length
        hex_decode(&v.as_bytes()[2..], &mut buffer)
            .map_err(|e| E::custom(format_args!("{:?}", e)))?;
        Ok(Byte65(buffer))
    }

    fn visit_string<E>(self, v: String) -> Result<Self::Value, E>
    where
        E: serde::de::Error,
    {
        self.visit_str(&v)
    }
}

impl serde::Serialize for Byte65 {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: serde::Serializer,
    {
        let mut buffer = [0u8; 132];
        buffer[0] = b'0';
        buffer[1] = b'x';
        hex_encode(&self.0, &mut buffer[2..])
            .map_err(|e| serde::ser::Error::custom(&format!("{}", e)))?;
        serializer.serialize_str(unsafe { ::std::str::from_utf8_unchecked(&buffer) })
    }
}

impl<'de> serde::Deserialize<'de> for Byte65 {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        deserializer.deserialize_str(Byte65Visitor)
    }
}

#[derive(Clone)]
pub struct Byte8(pub [u8; 8]);

impl Default for Byte8 {
    fn default() -> Self {
        Byte8([0u8; 8])
    }
}

impl PartialEq for Byte8 {
    fn eq(&self, other: &Byte8) -> bool {
        self.0[..] == other.0[..]
    }
}

impl Eq for Byte8 {}

impl Hash for Byte8 {
    fn hash<H: Hasher>(&self, state: &mut H) {
        state.write(&self.0);
    }
}

impl Debug for Byte8 {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        self.0.fmt(f)
    }
}

impl Byte8 {
    pub fn new(inner: [u8; 8]) -> Self {
        Byte8(inner)
    }
}

impl From<packed::Symbol> for Byte8 {
    fn from(packed: packed::Symbol) -> Self {
        let mut inner: [u8; 8] = [0u8; 8];
        inner.copy_from_slice(&packed.raw_data());
        Byte8(inner)
    }
}

impl From<Byte8> for packed::Symbol {
    fn from(json: Byte8) -> Self {
        Self::from_slice(&json.0).expect("impossible: fail to read inner array")
    }
}

struct Byte8Visitor;

impl<'b> serde::de::Visitor<'b> for Byte8Visitor {
    type Value = Byte8;

    fn expecting(&self, formatter: &mut fmt::Formatter) -> fmt::Result {
        write!(formatter, "a 0x-prefixed hex string")
    }

    fn visit_str<E>(self, v: &str) -> Result<Self::Value, E>
    where
        E: serde::de::Error,
    {
        if v.len() < 2 || &v.as_bytes()[0..2] != b"0x" || v.len() != 18 {
            return Err(E::invalid_value(serde::de::Unexpected::Str(v), &self));
        }
        let mut buffer = [0u8; 8]; // we checked length
        hex_decode(&v.as_bytes()[2..], &mut buffer)
            .map_err(|e| E::custom(format_args!("{:?}", e)))?;
        Ok(Byte8(buffer))
    }

    fn visit_string<E>(self, v: String) -> Result<Self::Value, E>
    where
        E: serde::de::Error,
    {
        self.visit_str(&v)
    }
}

impl serde::Serialize for Byte8 {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: serde::Serializer,
    {
        let mut buffer = [0u8; 18];
        buffer[0] = b'0';
        buffer[1] = b'x';
        hex_encode(&self.0, &mut buffer[2..])
            .map_err(|e| serde::ser::Error::custom(&format!("{}", e)))?;
        serializer.serialize_str(unsafe { ::std::str::from_utf8_unchecked(&buffer) })
    }
}

impl<'de> serde::Deserialize<'de> for Byte8 {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        deserializer.deserialize_str(Byte8Visitor)
    }
}
