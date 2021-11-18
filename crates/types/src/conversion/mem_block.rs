use sparse_merkle_tree::H256;

use crate::offchain::{CellInfo, DepositInfo};
use crate::{packed, prelude::*, vec::Vec};

impl Pack<packed::CellInfo> for CellInfo {
    fn pack(&self) -> packed::CellInfo {
        packed::CellInfo::new_builder()
            .out_point(self.out_point.to_owned())
            .output(self.output.to_owned())
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
            .request(self.request.to_owned())
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

impl_conversion_for_vector!(DepositInfo, DepositInfoVec, DepositInfoVecReader);
impl_conversion_for_option!(H256, Byte32Opt, Byte32OptReader);
