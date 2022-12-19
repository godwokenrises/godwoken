use gw_chain::chain::Chain;
use gw_common::merkle_utils::{calculate_ckb_merkle_root, ckb_merkle_leaf_hash};
use gw_smt::{
    smt::{Blake2bHasher, SMTH256},
    smt_h256_ext::SMTH256Ext,
};
use gw_types::{
    packed::{BlockMerkleState, GlobalState, L2Block, SubmitWithdrawals, WithdrawalRequest},
    prelude::{Builder, Entity, Pack, PackVec, Unpack},
};

pub fn generate_bad_block_using_first_withdrawal(
    chain: &Chain,
    block: L2Block,
    global_state: GlobalState,
) -> (L2Block, GlobalState) {
    let block = {
        let withdrawal = block.withdrawals().get_unchecked(0);
        let raw_withdrawal = withdrawal
            .raw()
            .as_builder()
            .account_script_hash([9u8; 32].pack())
            .build();
        let bad_withdrawal = withdrawal.as_builder().raw(raw_withdrawal).build();

        let mut withdrawals: Vec<WithdrawalRequest> = block.withdrawals().into_iter().collect();
        *withdrawals.get_mut(0).expect("exists") = bad_withdrawal;

        let withdrawal_witness_root = {
            let witnesses = withdrawals
                .iter()
                .enumerate()
                .map(|(idx, t)| ckb_merkle_leaf_hash(idx as u32, &t.witness_hash()));
            calculate_ckb_merkle_root(witnesses.collect())
        };

        let submit_withdrawals = SubmitWithdrawals::new_builder()
            .withdrawal_witness_root(withdrawal_witness_root.pack())
            .withdrawal_count((withdrawals.len() as u32).pack())
            .build();

        let raw_block = block
            .raw()
            .as_builder()
            .submit_withdrawals(submit_withdrawals)
            .build();

        block
            .as_builder()
            .raw(raw_block)
            .withdrawals(withdrawals.pack())
            .build()
    };

    let block_number = block.raw().number().unpack();
    let global_state = {
        let mut db = chain.store().begin_transaction();

        let bad_block_proof = db
            .block_smt()
            .unwrap()
            .merkle_proof(vec![SMTH256::from_u64(block_number)])
            .unwrap()
            .compile(vec![SMTH256::from_u64(block_number)])
            .unwrap();

        // Generate new block smt for global state
        let bad_block_smt = {
            let bad_block_root: [u8; 32] = bad_block_proof
                .compute_root::<Blake2bHasher>(vec![(block.smt_key().into(), block.hash().into())])
                .unwrap()
                .into();

            BlockMerkleState::new_builder()
                .merkle_root(bad_block_root.pack())
                .count((block_number + 1).pack())
                .build()
        };

        global_state
            .as_builder()
            .block(bad_block_smt)
            .tip_block_hash(block.hash().pack())
            .build()
    };

    (block, global_state)
}
