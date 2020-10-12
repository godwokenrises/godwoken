use crate::collector::Collector;
use crate::jsonrpc_types::collector::QueryParam;
use anyhow::{anyhow, Result};
use ckb_types::{
    bytes::Bytes,
    packed::{Script, Transaction, WitnessArgs, WitnessArgsReader},
    prelude::*,
};
use gw_generator::{
    smt::{Store, H256, SMT},
    syscalls::GetContractCode,
    Generator,
};
use gw_types::{
    packed::{L2Block, L2BlockReader, RawL2Block},
    prelude::Unpack as GWUnpack,
};

pub struct HeaderInfo {
    pub number: u64,
    pub block_hash: [u8; 32],
}

pub struct Chain<S, C, CS> {
    state: SMT<S>,
    collector: C,
    rollup_type_script: Script,
    last_synced: HeaderInfo,
    tip: RawL2Block,
    generator: Generator<CS>,
}

impl<S: Store<H256>, C: Collector, CS: GetContractCode> Chain<S, C, CS> {
    pub fn new(
        state: SMT<S>,
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
            let l2block = parse_l2block(
                &tx_info.transaction,
                self.rollup_type_script.calc_script_hash().unpack(),
            )?;

            let tip_number: u64 = self.tip.number().unpack();
            assert!(
                l2block.raw().number().unpack() == tip_number + 1,
                "new l2block number must be the successor of the tip"
            );

            // process l2block
            self.generator
                .apply_block_state(&mut self.state, &l2block)?;

            // update chain
            self.last_synced = HeaderInfo {
                number: header.raw().number().unpack(),
                block_hash: header.calc_header_hash().unpack(),
            };
            self.tip = l2block.raw();
        }
        Ok(())
    }
}

fn parse_l2block(tx: &Transaction, rollup_id: [u8; 32]) -> Result<L2Block> {
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
