use anyhow::{anyhow, Result};
use gw_common::{blake2b::new_blake2b, H256};
use gw_types::{
    core::DepType,
    packed::{Block, CellDep, Header, OutPoint},
    prelude::*,
};

#[derive(Debug, Clone)]
pub struct CKBGenesisInfo {
    header: Header,
    out_points: Vec<Vec<OutPoint>>,
    sighash_data_hash: H256,
    sighash_type_hash: H256,
    multisig_data_hash: H256,
    multisig_type_hash: H256,
    dao_data_hash: H256,
    dao_type_hash: H256,
}

impl CKBGenesisInfo {
    // Special cells in genesis transactions: (transaction-index, output-index)
    pub const SIGHASH_OUTPUT_LOC: (usize, usize) = (0, 1);
    pub const MULTISIG_OUTPUT_LOC: (usize, usize) = (0, 4);
    pub const DAO_OUTPUT_LOC: (usize, usize) = (0, 2);
    pub const SIGHASH_GROUP_OUTPUT_LOC: (usize, usize) = (1, 0);
    pub const MULTISIG_GROUP_OUTPUT_LOC: (usize, usize) = (1, 1);

    pub fn from_block(genesis_block: &Block) -> Result<Self> {
        let raw_header = genesis_block.header().raw();
        let number: u64 = raw_header.number().unpack();
        if number != 0 {
            return Err(anyhow!("Invalid genesis block number: {}", number));
        }

        let mut sighash_data_hash = None;
        let mut sighash_type_hash = None;
        let mut multisig_data_hash = None;
        let mut multisig_type_hash = None;
        let mut dao_data_hash = None;
        let mut dao_type_hash = None;
        let out_points = genesis_block
            .transactions()
            .into_iter()
            .enumerate()
            .map(|(tx_index, tx)| {
                let raw_tx = tx.raw();
                raw_tx
                    .outputs()
                    .into_iter()
                    .zip(raw_tx.outputs_data().into_iter())
                    .enumerate()
                    .map(|(index, (output, data))| {
                        let data_hash: H256 = {
                            let mut hasher = new_blake2b();
                            hasher.update(&data.raw_data());
                            let mut hash = [0u8; 32];
                            hasher.finalize(&mut hash);
                            hash.into()
                        };
                        if tx_index == Self::SIGHASH_OUTPUT_LOC.0
                            && index == Self::SIGHASH_OUTPUT_LOC.1
                        {
                            sighash_type_hash =
                                output.type_().to_opt().map(|script| script.hash().into());
                            sighash_data_hash = Some(data_hash);
                        }
                        if tx_index == Self::MULTISIG_OUTPUT_LOC.0
                            && index == Self::MULTISIG_OUTPUT_LOC.1
                        {
                            multisig_type_hash =
                                output.type_().to_opt().map(|script| script.hash().into());
                            multisig_data_hash = Some(data_hash);
                        }
                        if tx_index == Self::DAO_OUTPUT_LOC.0 && index == Self::DAO_OUTPUT_LOC.1 {
                            dao_type_hash =
                                output.type_().to_opt().map(|script| script.hash().into());
                            dao_data_hash = Some(data_hash);
                        }
                        let tx_hash = {
                            let mut hasher = new_blake2b();
                            hasher.update(tx.raw().as_slice());
                            let mut hash = [0u8; 32];
                            hasher.finalize(&mut hash);
                            hash
                        };
                        OutPoint::new_builder()
                            .tx_hash(tx_hash.pack())
                            .index((index as u32).pack())
                            .build()
                    })
                    .collect::<Vec<_>>()
            })
            .collect::<Vec<_>>();

        let sighash_data_hash =
            sighash_data_hash.ok_or_else(|| anyhow!("No data hash(sighash) found in txs[0][1]"))?;
        let sighash_type_hash =
            sighash_type_hash.ok_or_else(|| anyhow!("No type hash(sighash) found in txs[0][1]"))?;
        let multisig_data_hash = multisig_data_hash
            .ok_or_else(|| anyhow!("No data hash(multisig) found in txs[0][4]"))?;
        let multisig_type_hash = multisig_type_hash
            .ok_or_else(|| anyhow!("No type hash(multisig) found in txs[0][4]"))?;
        let dao_data_hash =
            dao_data_hash.ok_or_else(|| anyhow!("No data hash(dao) found in txs[0][2]"))?;
        let dao_type_hash =
            dao_type_hash.ok_or_else(|| anyhow!("No type hash(dao) found in txs[0][2]"))?;
        Ok(CKBGenesisInfo {
            header: genesis_block.header(),
            out_points,
            sighash_data_hash,
            sighash_type_hash,
            multisig_data_hash,
            multisig_type_hash,
            dao_data_hash,
            dao_type_hash,
        })
    }

    pub fn header(&self) -> &Header {
        &self.header
    }

    pub fn sighash_data_hash(&self) -> &H256 {
        &self.sighash_data_hash
    }

    pub fn sighash_type_hash(&self) -> &H256 {
        &self.sighash_type_hash
    }

    pub fn multisig_data_hash(&self) -> &H256 {
        &self.multisig_data_hash
    }

    pub fn multisig_type_hash(&self) -> &H256 {
        &self.multisig_type_hash
    }

    pub fn dao_data_hash(&self) -> &H256 {
        &self.dao_data_hash
    }

    pub fn dao_type_hash(&self) -> &H256 {
        &self.dao_type_hash
    }

    pub fn sighash_dep(&self) -> CellDep {
        CellDep::new_builder()
            .out_point(
                self.out_points[Self::SIGHASH_GROUP_OUTPUT_LOC.0][Self::SIGHASH_GROUP_OUTPUT_LOC.1]
                    .clone(),
            )
            .dep_type(DepType::DepGroup.into())
            .build()
    }

    pub fn multisig_dep(&self) -> CellDep {
        CellDep::new_builder()
            .out_point(
                self.out_points[Self::MULTISIG_GROUP_OUTPUT_LOC.0]
                    [Self::MULTISIG_GROUP_OUTPUT_LOC.1]
                    .clone(),
            )
            .dep_type(DepType::DepGroup.into())
            .build()
    }

    pub fn dao_dep(&self) -> CellDep {
        CellDep::new_builder()
            .out_point(self.out_points[Self::DAO_OUTPUT_LOC.0][Self::DAO_OUTPUT_LOC.1].clone())
            .build()
    }
}
