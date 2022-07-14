use sparse_merkle_tree::H256;

use crate::offchain::ExportedBlock;
use crate::{packed, prelude::*};

impl From<ExportedBlock> for packed::ExportedBlock {
    fn from(exported: ExportedBlock) -> Self {
        let deposit_asset_scripts = packed::ScriptVec::new_builder()
            .set(exported.deposit_asset_scripts)
            .build();

        packed::ExportedBlock::new_builder()
            .block(exported.block)
            .post_global_state(exported.post_global_state)
            .deposit_requests(exported.deposit_requests.pack())
            .deposit_asset_scripts(deposit_asset_scripts)
            .withdrawals(exported.withdrawals.pack())
            .bad_block_hashes(exported.bad_block_hashes.pack())
            .build()
    }
}

impl From<packed::ExportedBlock> for ExportedBlock {
    fn from(exported: packed::ExportedBlock) -> Self {
        let deposit_requests = exported.deposit_requests().into_iter().collect();
        let deposit_asset_scripts = exported.deposit_asset_scripts().into_iter().collect();
        let withdrawals = exported.withdrawals().into_iter().collect();

        ExportedBlock {
            block: exported.block(),
            post_global_state: exported.post_global_state(),
            deposit_requests,
            deposit_asset_scripts,
            withdrawals,
            bad_block_hashes: exported.bad_block_hashes().unpack(),
        }
    }
}

impl_conversion_for_vector!(Vec<H256>, Byte32VecVec, Byte32VecVecReader);
impl_conversion_for_option!(Vec<Vec<H256>>, Byte32VecVecOpt, Byte32VecVecOptReader);
