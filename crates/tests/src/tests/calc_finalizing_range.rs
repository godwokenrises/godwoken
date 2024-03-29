use crate::testing_tool::chain::setup_chain;
use gw_config::ForkConfig;
use gw_store::schema::{COLUMN_BLOCK, COLUMN_BLOCK_GLOBAL_STATE, COLUMN_INDEX};
use gw_store::traits::kv_store::KVStoreWrite;
use gw_types::core::Timepoint;
use gw_types::packed::{BlockMerkleState, L2Block, RawL2Block};
use gw_types::{packed::GlobalState, prelude::*};
use gw_utils::calc_finalizing_range;
use rand::Rng;

// Test gw_utils::calc_finalizing_range
#[tokio::test(flavor = "multi_thread", worker_threads = 1)]
async fn test_calc_finalizing_range() {
    // ## Prepare
    //
    // fork_config = {
    //   upgrade_global_state_version_to_v2: Some(100)
    // }
    //
    // rollup_config = {
    //   finality_blocks: DEFAULT_FINALITY_BLOCKS
    // }
    //
    // block[0].ts = 0
    // block[i].ts = block[i-1].ts + random(1, rollup_config.finality_time_in_ms())
    // global_state[i].last_finalized_timepoint = block[i].ts
    //
    // Assertions:
    // - No overlapped finalizing ranges
    // - For i <  100+6, { block[i].finalizing_range | j == i - 6 }
    // - For i >= 100+6, { block[i].finalizing_range | block[j].ts + finality_time_in_ms <= block[i].ts }

    let chain = setup_chain(Default::default()).await;
    let fork_config = ForkConfig {
        upgrade_global_state_version_to_v2: Some(100),
        ..Default::default()
    };
    let rollup_config = chain.generator().rollup_context().rollup_config.clone();
    let blocks = {
        let mut rng = rand::thread_rng();
        let mut parent_timestamp = 0u64;
        let mut parent_hash: [u8; 32] = Default::default();
        (0..=fork_config.upgrade_global_state_version_to_v2.unwrap() * 2)
            .map(|number| {
                let timestamp =
                    parent_timestamp + rng.gen_range(1..rollup_config.finality_time_in_ms());
                let raw = RawL2Block::new_builder()
                    .number(number.pack())
                    .timestamp(timestamp.pack())
                    .parent_block_hash(parent_hash.pack())
                    .build();
                let l2block = L2Block::new_builder().raw(raw).build();

                parent_timestamp = timestamp;
                parent_hash = l2block.hash();

                l2block
            })
            .collect::<Vec<_>>()
    };
    let global_states = blocks
        .iter()
        .map(|block| {
            let number = block.raw().number().unpack();
            let timestamp = block.raw().timestamp().unpack();
            let version = if Some(number) < fork_config.upgrade_global_state_version_to_v2 {
                1u8
            } else {
                2u8
            };
            let block_count = number + 1;
            let last_finalized_timepoint = if version <= 1 {
                let finality_as_blocks = rollup_config.finality_blocks().unpack();
                Timepoint::from_block_number(number.saturating_sub(finality_as_blocks))
            } else {
                Timepoint::from_timestamp(timestamp)
            };
            GlobalState::new_builder()
                .version(version.into())
                .block(
                    BlockMerkleState::new_builder()
                        .count(block_count.pack())
                        .build(),
                )
                .tip_block_timestamp(timestamp.pack())
                .last_finalized_timepoint(last_finalized_timepoint.full_value().pack())
                .build()
        })
        .collect::<Vec<_>>();

    // ## Process Blocks Finalizing Range
    for (block, global_state) in blocks.iter().zip(global_states.iter()) {
        let raw = block.raw();
        let mut db = chain.store().begin_transaction();
        db.insert_raw(
            COLUMN_BLOCK_GLOBAL_STATE,
            block.hash().as_slice(),
            global_state.as_slice(),
        )
        .unwrap();
        db.insert_raw(COLUMN_INDEX, raw.number().as_slice(), &block.hash())
            .unwrap();
        db.insert_raw(COLUMN_BLOCK, &block.hash(), block.as_slice())
            .unwrap();
        db.commit().unwrap();
    }

    // ## Assert
    let mut last_range_end = None;
    let fork_height = fork_config.upgrade_global_state_version_to_v2.unwrap();
    let finality_as_blocks = rollup_config.finality_blocks().unpack();
    let finality_time_in_mss = rollup_config.finality_time_in_ms();
    for i in 1..blocks.len() {
        let block = &blocks[i];
        let block_number = block.raw().number().unpack();
        let block_timestamp = block.raw().timestamp().unpack();
        let range =
            calc_finalizing_range(&rollup_config, &fork_config, chain.store(), block).unwrap();

        if block_number < finality_as_blocks {
            assert!(range.is_empty());
        } else {
            for nb in range.clone() {
                if nb < fork_height {
                    assert_eq!(nb, block_number.saturating_sub(finality_as_blocks));
                } else {
                    let ts = blocks[nb as usize].raw().timestamp().unpack();
                    assert!(ts + finality_time_in_mss <= block_timestamp);
                }
            }
        }

        if !range.is_empty() {
            assert!(range.start < range.end);
            if last_range_end.is_none() {
                assert_eq!(
                    range,
                    (1..2),
                    "block_number: {}, range: {:?}, last_range_end: {:?}",
                    block_number,
                    range,
                    last_range_end
                );
            } else {
                assert_eq!(
                    last_range_end.unwrap(),
                    range.start,
                    "block_number: {}, range: {:?}, last_range_end: {:?}",
                    block_number,
                    range,
                    last_range_end
                );
            }
            last_range_end = Some(range.end);
        }
    }
}
