use anyhow::Result;
use gw_common::registry_address::RegistryAddress;
use gw_common::state::State;
use gw_generator::traits::StateExt;
use gw_store::state::traits::JournalDB;
use gw_traits::CodeStore;
use gw_types::h256::*;
use gw_types::packed::Script;

pub fn mock_account<S: State + CodeStore + JournalDB>(
    state: &mut S,
    registry_address: RegistryAddress,
    account_script: Script,
) -> Result<u32> {
    let account_script_hash: H256 = account_script.hash();
    if let Some(account_id) = state.get_account_id_by_script_hash(&account_script_hash)? {
        return Ok(account_id);
    }

    let account_id = state.create_account_from_script(account_script)?;
    state.mapping_registry_address_to_script_hash(registry_address, account_script_hash)?;
    Ok(account_id)
}
