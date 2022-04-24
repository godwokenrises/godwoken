use std::convert::TryInto;

use crate::{
    error::Error,
    registry_address::RegistryAddress,
    state::{
        build_registry_address_to_script_hash_key, build_script_hash_to_registry_address_key, State,
    },
};
use sparse_merkle_tree::H256;

pub trait StateTest: State {
    fn mapping_address(
        &mut self,
        address: RegistryAddress,
        script_hash: H256,
    ) -> Result<(), Error> {
        // script_hash -> address
        let key = build_script_hash_to_registry_address_key(&script_hash);
        let value: [u8; 32] = address.to_bytes().try_into().expect("buffer overflow");
        self.update_value(address.registry_id, &key, value.into())?;
        // address -> script
        let key = build_registry_address_to_script_hash_key(&address);
        self.update_value(address.registry_id, &key, script_hash)?;
        Ok(())
    }
}
