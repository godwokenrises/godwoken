use std::{convert::TryInto, time::Instant};

use crate::setup::Context as ChainContext;
use anyhow::{anyhow, Result};
use gw_store::{
    state::{history::history_state::RWConfig, BlockStateDB},
    traits::chain_store::ChainStore,
};
use gw_types::{core::ChallengeTargetType, packed::Byte32, prelude::*};

pub fn replay_chain(ctx: ChainContext) -> Result<()> {
    let ChainContext {
        mut chain,
        from_store,
        local_store,
    } = ctx;
    let tip = local_store.get_tip_block()?;
    let number = {
        let block_hash = from_store.get_block_hash_by_number(tip.raw().number().unpack())?;
        assert_eq!(tip.hash(), block_hash.unwrap());
        tip.raw().number().unpack()
    };

    let hash: Byte32 = tip.hash().pack();
    println!("Replay from block: #{} {}", number, hash);

    // query next block
    let mut replay_number = number + 1;

    loop {
        let now = Instant::now();
        let block_hash = match from_store.get_block_hash_by_number(replay_number)? {
            Some(block_hash) => block_hash,
            None => {
                println!("Can't find block #{}, stop replay", replay_number);
                break;
            }
        };

        let block = from_store.get_block(&block_hash)?.expect("block");
        let block_number: u64 = block.raw().number().unpack();
        assert_eq!(block_number, replay_number, "number should be consist");
        let global_state = from_store
            .get_block_post_global_state(&block.raw().parent_block_hash().unpack())?
            .expect("block prev global state");
        let deposit_requests = from_store
            .get_block_deposit_info_vec(block_number)
            .expect("block deposit info vec");
        let withdrawals = block
            .withdrawals()
            .into_iter()
            .map(|withdrawal| {
                from_store
                    .get_withdrawal(&withdrawal.hash())
                    .expect("query")
                    .expect("block deposit requests")
            })
            .collect();
        let load_block_ms = now.elapsed().as_millis();

        let txs_len = block.transactions().item_count();
        let deposits_len = deposit_requests.len();
        let mut db = local_store.begin_transaction();
        let now = Instant::now();
        if let Some(challenge) = chain.process_block(
            &mut db,
            block,
            global_state,
            deposit_requests,
            Default::default(),
            withdrawals,
        )? {
            let target_type: u8 = challenge.target_type().into();
            let target_type: ChallengeTargetType = target_type.try_into().unwrap();
            let target_index: u32 = challenge.target_index().unpack();
            println!(
                "Challenge found type: {:?} index: {}",
                target_type, target_index
            );
            return Err(anyhow!("challenge found"));
        }

        let process_block_ms = now.elapsed().as_millis();

        let now = Instant::now();
        db.commit()?;
        let db_commit_ms = now.elapsed().as_millis();

        println!(
            "Replay block: #{} {} (txs: {} deposits: {} process time: {}ms commit time: {}ms load time: {}ms)",
            replay_number,
            {
                let hash: Byte32 = block_hash.pack();
                hash
            },
            txs_len,
            deposits_len,
            process_block_ms,
            db_commit_ms,
            load_block_ms
        );
        local_store.check_state()?;

        replay_number += 1;
    }
    Ok(())
}

pub fn detach_chain(ctx: ChainContext) -> Result<()> {
    let ChainContext {
        chain: _,
        from_store: _,
        local_store,
    } = ctx;
    let tip = local_store.get_tip_block()?;
    let mut number = tip.raw().number().unpack();
    let hash: Byte32 = tip.hash().pack();
    println!("Detach from block: #{} {}", number, hash);

    // query next block
    while number > 0 {
        let mut db = local_store.begin_transaction();
        let detach_block = {
            let block_hash = db.get_block_hash_by_number(number)?.unwrap();
            db.get_block(&block_hash)?.unwrap()
        };
        let hash: Byte32 = detach_block.hash().pack();
        number = detach_block.raw().number().unpack();
        println!("Detach block: #{} {}", number, hash);
        db.detach_block(&detach_block)?;
        {
            let mut state = BlockStateDB::from_store(&mut db, RWConfig::detach_block())?;
            state.detach_block_state(number)?;
        }
        db.commit()?;
        local_store.check_state()?;
        number -= 1;
    }
    Ok(())
}
