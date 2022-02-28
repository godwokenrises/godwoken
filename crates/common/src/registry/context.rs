use crate::vec::Vec;
use core::convert::TryInto;

use gw_types::{
    core::AllowedEoaType,
    packed::{AllowedTypeHash, Byte32},
};

use crate::{error::Error, registry_address::RegistryAddress};

pub struct RegistryContext {
    allowed_eoa_type_hashes: Vec<AllowedTypeHash>,
}

impl RegistryContext {
    pub fn new(allowed_eoa_type_hashes: Vec<AllowedTypeHash>) -> Self {
        Self {
            allowed_eoa_type_hashes,
        }
    }
    fn find_eoa_type_by_hash(&self, code_hash: &Byte32) -> Option<&AllowedTypeHash> {
        self.allowed_eoa_type_hashes
            .iter()
            .find(|type_hash| &type_hash.hash() == code_hash)
    }

    /// Extract EOA registry address from deposit request
    // TODO support extract ETH address from tron EOA
    pub fn extract_registry_address_from_deposit(
        &self,
        registry_id: u32,
        code_hash: &Byte32,
        args: &[u8],
    ) -> Result<RegistryAddress, Error> {
        // Check EOA code hash
        match self
            .find_eoa_type_by_hash(code_hash)
            .map(|type_hash| {
                let type_: u8 = type_hash.type_().into();
                type_.try_into()
            })
            .transpose()
            .map_err(|_err| Error::UnknownEoaCodeHash)?
        {
            Some(AllowedEoaType::Eth) => {
                // extract ETH EOA
                let address =
                    { crate::registry::eth_registry::extract_eth_address_from_eoa(&args)? };
                let addr = RegistryAddress::new(registry_id, address);
                Ok(addr)
            }
            _ => Err(Error::UnknownEoaCodeHash),
        }
    }
}
