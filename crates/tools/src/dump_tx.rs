use crate::godwoken_rpc::GodwokenRpcClient;

use ckb_fixed_hash::H256;
use gw_jsonrpc_types::{debugger::DumpChallengeTarget, godwoken::ChallengeTargetType};

use std::{fs::write, path::Path, str::FromStr};

pub enum ChallengeBlock {
    Number(u64),
    Hash(H256),
}

impl FromStr for ChallengeBlock {
    type Err = String;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        if let Ok(n) = u64::from_str(s) {
            return Ok(ChallengeBlock::Number(n));
        }

        match H256::from_str(s) {
            Ok(h) => Ok(ChallengeBlock::Hash(h)),
            Err(_) => Err("invalid challenge block, must be number or h256".to_string()),
        }
    }
}

pub fn dump_tx(
    godwoken_rpc_url: &str,
    block: ChallengeBlock,
    target_index: u32,
    target_type: ChallengeTargetType,
    output: &Path,
) -> Result<(), String> {
    let challenge_target = match block {
        ChallengeBlock::Number(block_number) => DumpChallengeTarget::ByBlockNumber {
            block_number: block_number.into(),
            target_index: target_index.into(),
            target_type,
        },
        ChallengeBlock::Hash(block_hash) => DumpChallengeTarget::ByBlockHash {
            block_hash,
            target_index: target_index.into(),
            target_type,
        },
    };

    let mut godwoken_rpc_client = GodwokenRpcClient::new(godwoken_rpc_url);
    let dump_tx = godwoken_rpc_client.debug_dump_cancel_challenge_tx(challenge_target)?;

    let json_tx = serde_json::to_string_pretty(&dump_tx).map_err(|err| err.to_string())?;
    write(output, json_tx).map_err(|err| err.to_string())?;

    Ok(())
}
