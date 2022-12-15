use anyhow::Error;
use gw_db::{
    schema::{COLUMN_BLOCK_STATE_RECORD, COLUMN_BLOCK_STATE_REVERSE_RECORD},
    DBRawIterator, Direction, IteratorMode,
};
use gw_types::h256::*;

use crate::{
    state::history::{
        block_state_record::{BlockStateRecordKey, BlockStateRecordKeyReverse},
        history_state::HistoryStateStore,
    },
    traits::kv_store::{KVStoreRead, KVStoreWrite},
};

use super::StoreTransaction;

impl HistoryStateStore for &StoreTransaction {
    type BlockStateRecordKeyIter = Vec<BlockStateRecordKey>;

    fn iter_block_state_record(&self, block_number: u64) -> Self::BlockStateRecordKeyIter {
        let start_key = BlockStateRecordKey::new(block_number, &H256::zero());
        self.get_iter(
            COLUMN_BLOCK_STATE_RECORD,
            IteratorMode::From(start_key.as_slice(), Direction::Forward),
        )
        .map(|(key, _value)| BlockStateRecordKey::from_slice(&key))
        .take_while(move |key| key.block_number() == block_number)
        .collect()
    }

    fn remove_block_state_record(&self, block_number: u64) -> Result<(), Error> {
        let iter = self.iter_block_state_record(block_number);
        for record_key in iter {
            // delete record key
            self.delete(COLUMN_BLOCK_STATE_RECORD, record_key.as_slice())?;
            // delete reverse record key
            let reverse_key =
                BlockStateRecordKeyReverse::new(record_key.block_number(), &record_key.state_key());
            self.delete(COLUMN_BLOCK_STATE_REVERSE_RECORD, reverse_key.as_slice())?;
        }
        Ok(())
    }

    fn get_history_state(&self, block_number: u64, state_key: &H256) -> Option<H256> {
        let key = BlockStateRecordKeyReverse::new(block_number, state_key);
        let mut raw_iter: DBRawIterator = self
            .get_iter(COLUMN_BLOCK_STATE_REVERSE_RECORD, IteratorMode::Start)
            .into();
        raw_iter.seek_for_prev(key.as_slice());

        if !raw_iter.valid() {
            return None;
        }
        match raw_iter.key() {
            Some(prev_key) => {
                // not a some key
                if &prev_key[..32] != key.state_key().as_slice() {
                    return None;
                }

                // get old value
                let prev_reverse_key = BlockStateRecordKeyReverse::from_slice(prev_key);
                let prev_key = BlockStateRecordKey::new(
                    prev_reverse_key.block_number(),
                    &prev_reverse_key.state_key(),
                );

                self.get(COLUMN_BLOCK_STATE_RECORD, prev_key.as_slice())
                    .map(|raw| {
                        let mut buf = [0u8; 32];
                        buf.copy_from_slice(&raw);
                        buf
                    })
            }
            _ => None,
        }
    }

    fn record_block_state(
        &self,
        block_number: u64,
        state_key: H256,
        value: H256,
    ) -> Result<(), Error> {
        // insert block state key value
        let key = BlockStateRecordKey::new(block_number, &state_key);
        self.insert_raw(COLUMN_BLOCK_STATE_RECORD, key.as_slice(), value.as_slice())?;
        // insert block state reverse key
        let reverse_key = BlockStateRecordKeyReverse::new(block_number, &state_key);
        self.insert_raw(
            COLUMN_BLOCK_STATE_REVERSE_RECORD,
            reverse_key.as_slice(),
            &[],
        )?;
        Ok(())
    }
}
