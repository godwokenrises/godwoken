use sparse_merkle_tree::H256;

use crate::offchain::{CellInfo, DepositInfo, SudtCustodian};
use crate::{packed, prelude::*, vec::Vec};

impl Pack<packed::CellInfo> for CellInfo {
    fn pack(&self) -> packed::CellInfo {
        packed::CellInfo::new_builder()
            .out_point(self.out_point.clone())
            .output(self.output.clone())
            .data(self.data.pack())
            .build()
    }
}

impl<'r> Unpack<CellInfo> for packed::CellInfoReader<'r> {
    fn unpack(&self) -> CellInfo {
        CellInfo {
            out_point: self.out_point().to_entity(),
            output: self.output().to_entity(),
            data: self.data().unpack(),
        }
    }
}

impl Pack<packed::DepositInfo> for DepositInfo {
    fn pack(&self) -> packed::DepositInfo {
        packed::DepositInfo::new_builder()
            .request(self.request.clone())
            .cell(self.cell.pack())
            .build()
    }
}

impl<'r> Unpack<DepositInfo> for packed::DepositInfoReader<'r> {
    fn unpack(&self) -> DepositInfo {
        DepositInfo {
            request: self.request().to_entity(),
            cell: self.cell().unpack(),
        }
    }
}

impl Pack<packed::SudtCustodian> for SudtCustodian {
    fn pack(&self) -> packed::SudtCustodian {
        packed::SudtCustodian::new_builder()
            .script_hash(self.script_hash.pack())
            .amount(self.amount.pack())
            .script(self.script.clone())
            .build()
    }
}

impl<'r> Unpack<SudtCustodian> for packed::SudtCustodianReader<'r> {
    fn unpack(&self) -> SudtCustodian {
        SudtCustodian {
            script_hash: self.script_hash().unpack(),
            amount: self.amount().unpack(),
            script: self.script().to_entity(),
        }
    }
}

impl_conversion_for_packed_iterator_pack!(AccountMerkleState, AccountMerkleStateVec);
impl_conversion_for_vector!(DepositInfo, DepositInfoVec, DepositInfoVecReader);
impl_conversion_for_vector!(SudtCustodian, SudtCustodianVec, SudtCustodianVecReader);
impl_conversion_for_packed_iterator_pack!(WithdrawalRequestExtra, WithdrawalRequestExtraVec);
impl_conversion_for_packed_iterator_pack!(DepositInfo, DepositInfoVec);
impl_conversion_for_option!(H256, Byte32Opt, Byte32OptReader);
