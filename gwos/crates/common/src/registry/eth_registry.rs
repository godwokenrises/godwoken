#![allow(dead_code)]

use crate::error::Error;
use crate::vec::Vec;

const EOA_SCRIPT_ARGS_LEN: usize = 52;
/// 32 + 4 + 20
const CONTRACT_ACCOUNT_SCRIPT_ARGS_LEN: usize = 56;

/// Extract ETH address from an ETH EOA script args
pub fn extract_eth_address_from_eoa(script_args: &[u8]) -> Result<Vec<u8>, Error> {
    if script_args.len() != EOA_SCRIPT_ARGS_LEN {
        return Err(Error::InvalidArgs);
    }
    Ok(script_args[32..].to_vec())
}

/// Extract ETH address from an ETH contract script args
pub fn extract_eth_address_from_contract(script_args: &[u8]) -> Result<Vec<u8>, Error> {
    if script_args.len() != CONTRACT_ACCOUNT_SCRIPT_ARGS_LEN {
        return Err(Error::InvalidArgs);
    }
    Ok(script_args[36..].to_vec())
}
