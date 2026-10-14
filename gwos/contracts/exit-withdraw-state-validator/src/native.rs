//! Version-pinned native state rules. The on-chain entry point chooses the version.
use crate::*;
use alloc::collections::BTreeSet;
use gw_common::state::*;
use gw_hash::blake2b::hash;
use gw_types::{registry_address::RegistryAddress, U256};
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum NativeVersion {
    V1,
    V0,
}
pub const RAW_DESTINATION: u32 = u32::MAX;
pub fn short_key(address: [u8; 20]) -> Hash {
    let mut b = [0u8; 29];
    b[4] = 5;
    b[5..9].copy_from_slice(&20u32.to_le_bytes());
    b[9..].copy_from_slice(&address);
    hash(&b)
}
impl NativeVersion {
    pub fn balance_key(self, id: u32, address: [u8; 20]) -> Hash {
        if self == Self::V1 {
            return crate::balance_key(id, address);
        }
        let mut b = [0u8; 33];
        b[..4].copy_from_slice(&id.to_le_bytes());
        b[5..9].copy_from_slice(&1u32.to_le_bytes());
        b[9..13].copy_from_slice(&20u32.to_le_bytes());
        b[13..].copy_from_slice(&address);
        hash(&b)
    }
    pub fn supply_key(self, id: u32) -> Option<Hash> {
        if self == Self::V1 {
            Some(crate::supply_key(id))
        } else {
            None
        }
    }
    pub fn amount(self, v: Hash) -> Result<U256> {
        verify::require(self == Self::V1 || v[16..] == [0; 16], Error::Balance)?;
        Ok(crate::amount(v))
    }
    pub fn scale(self) -> u64 {
        if self == Self::V1 {
            SCALE
        } else {
            1
        }
    }
    pub fn protected(self, s: &settlement::records::Source) -> bool {
        s.address == [0; 20] || (self == Self::V0 && s.account == RAW_DESTINATION)
    }
    pub fn identity_keys(self, id: u32, sh: Hash, address: [u8; 20], keys: &mut BTreeSet<Hash>) {
        if self == Self::V0 {
            keys.insert(short_key(address));
            if id != RAW_DESTINATION {
                keys.insert(build_account_field_key(id, GW_ACCOUNT_SCRIPT_HASH_TYPE));
                keys.insert(build_script_hash_to_account_id_key(&sh));
            }
        } else {
            let r = RegistryAddress::new(2, address.to_vec());
            keys.extend([
                build_account_field_key(id, GW_ACCOUNT_SCRIPT_HASH_TYPE),
                build_script_hash_to_account_id_key(&sh),
                build_account_key(2, &build_registry_address_to_script_hash_key(&r)),
                build_account_key(2, &build_script_hash_to_registry_address_key(&sh)),
            ]);
        }
    }
    pub fn identity(
        self,
        id: u32,
        sh: Hash,
        address: [u8; 20],
        count: u32,
        mut read: impl FnMut(Hash) -> Result<Hash>,
    ) -> Result<()> {
        use verify::require;
        if self == Self::V0 && id == RAW_DESTINATION {
            require(
                sh == [0; 32] && address != [0; 20] && read(short_key(address))? == [0; 32],
                Error::Recipient,
            )?;
            return Ok(());
        }
        require(id < count && sh != [0; 32], Error::Recipient)?;
        require(
            read(build_account_field_key(id, GW_ACCOUNT_SCRIPT_HASH_TYPE))? == sh,
            Error::Recipient,
        )?;
        let mut encoded = [0; 32];
        encoded[..4].copy_from_slice(&id.to_le_bytes());
        // v0 stores a plain LE32 account ID; v1 adds an existence flag.
        if self == Self::V1 {
            encoded[4] = 1;
        }
        require(
            read(build_script_hash_to_account_id_key(&sh))? == encoded,
            Error::Recipient,
        )?;
        if self == Self::V0 {
            require(
                address == sh[..20] && read(short_key(address))? == sh,
                Error::Recipient,
            )
        } else {
            let r = RegistryAddress::new(2, address.to_vec());
            require(
                read(build_account_key(
                    2,
                    &build_registry_address_to_script_hash_key(&r),
                ))? == sh,
                Error::Recipient,
            )?;
            let mut reg = [0; 32];
            r.write_to_slice(&mut reg).map_err(|_| Error::Recipient)?;
            require(
                read(build_account_key(
                    2,
                    &build_script_hash_to_registry_address_key(&sh),
                ))? == reg,
                Error::Recipient,
            )
        }
    }
}
