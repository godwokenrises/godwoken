use crate::collector::Collector;
use crate::jsonrpc_types::collector::QueryParam;
use anyhow::{anyhow, Result};
use ckb_types::{
    bytes::Bytes,
    core::ScriptHashType,
    packed::{CellOutput, RawTransaction, Script, Transaction, WitnessArgs, WitnessArgsReader},
    prelude::*,
};
use gw_common::{
    smt::{Store, H256, SMT},
    state::State,
    CKB_TOKEN_ID, DEPOSITION_CODE_HASH, SUDT_CODE_HASH,
};
use gw_generator::{
    generator::{DepositionRequest, StateTransitionArgs},
    syscalls::GetContractCode,
    Generator,
};
use gw_types::{
    packed::{DepositionLockArgs, DepositionLockArgsReader, L2Block, L2BlockReader, RawL2Block},
    prelude::Unpack as GWUnpack,
};

pub struct HeaderInfo {
    pub number: u64,
    pub block_hash: [u8; 32],
}

pub struct Chain<S, C, CS> {
    state: S,
    collector: C,
    rollup_type_script: Script,
    last_synced: HeaderInfo,
    tip: RawL2Block,
    generator: Generator<CS>,
}

impl<S: State, C: Collector, CS: GetContractCode> Chain<S, C, CS> {
    pub fn new(
        state: S,
        tip: RawL2Block,
        last_synced: HeaderInfo,
        rollup_type_script: Script,
        collector: C,
        code_store: CS,
    ) -> Self {
        let generator = Generator::new(code_store);
        Chain {
            state,
            collector,
            rollup_type_script,
            last_synced,
            tip,
            generator,
        }
    }

    /// Sync chain from layer1
    pub fn sync(&mut self) -> Result<()> {
        // TODO handle rollback
        if self
            .collector
            .get_header(&self.last_synced.block_hash)?
            .is_none()
        {
            panic!("layer1 chain has forked!")
        }
        // query state update tx from collector
        let param = QueryParam {
            type_: Some(self.rollup_type_script.clone().into()),
            from_block: Some(self.last_synced.number.into()),
            ..Default::default()
        };
        let txs = self.collector.query_transactions(param)?;
        // apply tx to state
        for tx_info in txs {
            let header = self
                .collector
                .get_header(&tx_info.block_hash)?
                .expect("should not panic unless the chain is forking");
            let block_number: u64 = header.raw().number().unpack();
            assert!(
                block_number > self.last_synced.number,
                "must greater than last synced number"
            );

            // parse layer2 block
            let rollup_id = self.rollup_type_script.calc_script_hash().unpack();
            let l2block = parse_l2block(&tx_info.transaction, &rollup_id)?;

            let tip_number: u64 = self.tip.number().unpack();
            assert!(
                l2block.raw().number().unpack() == tip_number + 1,
                "new l2block number must be the successor of the tip"
            );

            // process l2block
            self.process_block(l2block.clone(), &tx_info.transaction.raw(), &rollup_id)?;

            // update chain
            self.last_synced = HeaderInfo {
                number: header.raw().number().unpack(),
                block_hash: header.calc_header_hash().unpack(),
            };
            self.tip = l2block.raw();
        }
        Ok(())
    }

    fn process_block(
        &mut self,
        l2block: L2Block,
        tx: &RawTransaction,
        rollup_id: &[u8; 32],
    ) -> Result<()> {
        let deposition_requests = collect_deposition_requests(&self.collector, tx, rollup_id)?;
        let args = StateTransitionArgs {
            l2block,
            deposition_requests,
        };
        self.generator
            .apply_state_transition(&mut self.state, args)?;
        Ok(())
    }
}

fn parse_l2block(tx: &Transaction, rollup_id: &[u8; 32]) -> Result<L2Block> {
    // find rollup state cell from outputs
    let (i, _) = tx
        .raw()
        .outputs()
        .into_iter()
        .enumerate()
        .find(|(_i, output)| {
            output
                .type_()
                .to_opt()
                .map(|type_| type_.calc_script_hash().unpack())
                .as_ref()
                == Some(rollup_id)
        })
        .ok_or_else(|| anyhow!("no rollup cell found"))?;

    let witness: Bytes = tx
        .witnesses()
        .get(i)
        .ok_or_else(|| anyhow!("no witness"))?
        .unpack();
    let witness_args = match WitnessArgsReader::verify(&witness, false) {
        Ok(_) => WitnessArgs::new_unchecked(witness),
        Err(_) => {
            return Err(anyhow!("invalid witness"));
        }
    };
    let output_type: Bytes = witness_args
        .output_type()
        .to_opt()
        .ok_or_else(|| anyhow!("output_type field is none"))?
        .unpack();
    match L2BlockReader::verify(&output_type, false) {
        Ok(_) => Ok(L2Block::new_unchecked(output_type)),
        Err(_) => Err(anyhow!("invalid l2block")),
    }
}

fn collect_deposition_requests<C: Collector>(
    collector: &C,
    tx: &RawTransaction,
    rollup_id: &[u8; 32],
) -> Result<Vec<DepositionRequest>> {
    let mut deposition_requests = Vec::with_capacity(tx.inputs().len());
    // find deposition requests
    for (i, cell_input) in tx.inputs().into_iter().enumerate() {
        let previous_tx =
            collector.get_transaction(&cell_input.previous_output().tx_hash().unpack())?;
        let cell = previous_tx.transaction.raw().outputs().get(i).expect("get");
        let cell_data: Bytes = previous_tx
            .transaction
            .raw()
            .outputs_data()
            .get(i)
            .map(|data| data.unpack())
            .unwrap_or_default();
        let lock = cell.lock();
        let lock_code_hash: [u8; 32] = lock.code_hash().unpack();
        // not a deposition request lock
        if !(lock.hash_type() == ScriptHashType::Data.into()
            && lock_code_hash == DEPOSITION_CODE_HASH)
        {
            continue;
        }
        let args: Bytes = lock.args().unpack();
        let deposition_args = match DepositionLockArgsReader::verify(&args, false) {
            Ok(_) => DepositionLockArgs::new_unchecked(args),
            Err(_) => {
                return Err(anyhow!("invalid deposition request"))?;
            }
        };

        // ignore deposition request that do not belong to Rollup
        if &deposition_args.rollup_type_id().unpack() != rollup_id {
            continue;
        }

        // get token_id
        let token_id = fetch_token_id(cell.type_().to_opt())?;
        let value = fetch_sudt_value(&token_id, &cell, &cell_data);
        let deposition_request = DepositionRequest {
            token_id,
            value,
            pubkey_hash: deposition_args.pubkey_hash().unpack(),
            account_id: deposition_args.account_id().unpack(),
        };
        deposition_requests.push(deposition_request);
    }
    Ok(deposition_requests)
}

fn fetch_token_id(type_: Option<Script>) -> Result<[u8; 32]> {
    match type_ {
        Some(type_) => {
            let code_hash: [u8; 32] = type_.code_hash().unpack();
            if type_.hash_type() == ScriptHashType::Data.into() && code_hash == SUDT_CODE_HASH {
                return Ok(type_.calc_script_hash().unpack());
            }
            return Err(anyhow!("invalid SUDT token"));
        }
        None => Ok(CKB_TOKEN_ID),
    }
}

fn fetch_sudt_value(token_id: &[u8; 32], output: &CellOutput, data: &[u8]) -> u128 {
    if token_id == &CKB_TOKEN_ID {
        let capacity: u64 = output.capacity().unpack();
        return capacity.into();
    }
    let mut buf = [0u8; 16];
    buf.copy_from_slice(&data[..16]);
    u128::from_le_bytes(buf)
}
