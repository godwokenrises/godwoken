use crate::vec::Vec;
use core::convert::TryInto;

#[derive(Debug, Default, Clone, PartialEq, Eq, Hash)]
pub struct RegistryAddress {
    pub registry_id: u32,
    pub address: Vec<u8>,
}

impl RegistryAddress {
    pub fn new(registry_id: u32, address: Vec<u8>) -> Self {
        Self {
            registry_id,
            address,
        }
    }

    pub fn from_slice(slice: &[u8]) -> Option<Self> {
        if slice.len() < 8 {
            return None;
        }
        let registry_id = u32::from_le_bytes(slice[..4].try_into().unwrap());
        let address_len = u32::from_le_bytes(slice[4..8].try_into().unwrap());
        if slice.len() < address_len.checked_add(8)? as usize {
            return None;
        }
        let reg_addr = RegistryAddress {
            registry_id,
            address: slice[8..(8 + address_len as usize)].to_vec(),
        };
        Some(reg_addr)
    }

    pub fn len(&self) -> usize {
        8 + self.address.len()
    }

    pub fn is_empty(&self) -> bool {
        self.address.len() == 0
    }

    pub fn write_to_slice(&self, buf: &mut [u8]) -> Result<usize, usize> {
        if self.len() > buf.len() || self.len() > u32::MAX as usize {
            return Err(self.len());
        }
        buf[..4].copy_from_slice(&self.registry_id.to_le_bytes());
        buf[4..8].copy_from_slice(&(self.address.len() as u32).to_le_bytes());
        buf[8..(8 + self.address.len())].copy_from_slice(&self.address);
        Ok(self.len())
    }

    pub fn to_bytes(&self) -> Vec<u8> {
        let mut buf = Vec::default();
        buf.resize(self.len(), 0);
        self.write_to_slice(&mut buf).unwrap();
        buf
    }
}
