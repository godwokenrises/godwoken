// Generated by Molecule 0.7.2

use super::blockchain::*;
use super::godwoken::*;
use super::mem_block::*;
use super::store::*;
use molecule::prelude::*;
#[derive(Clone)]
pub struct ExportedBlock(molecule::bytes::Bytes);
impl ::core::fmt::LowerHex for ExportedBlock {
    fn fmt(&self, f: &mut ::core::fmt::Formatter) -> ::core::fmt::Result {
        use molecule::hex_string;
        if f.alternate() {
            write!(f, "0x")?;
        }
        write!(f, "{}", hex_string(self.as_slice()))
    }
}
impl ::core::fmt::Debug for ExportedBlock {
    fn fmt(&self, f: &mut ::core::fmt::Formatter) -> ::core::fmt::Result {
        write!(f, "{}({:#x})", Self::NAME, self)
    }
}
impl ::core::fmt::Display for ExportedBlock {
    fn fmt(&self, f: &mut ::core::fmt::Formatter) -> ::core::fmt::Result {
        write!(f, "{} {{ ", Self::NAME)?;
        write!(f, "{}: {}", "block", self.block())?;
        write!(f, ", {}: {}", "post_global_state", self.post_global_state())?;
        write!(f, ", {}: {}", "deposit_info_vec", self.deposit_info_vec())?;
        write!(
            f,
            ", {}: {}",
            "deposit_asset_scripts",
            self.deposit_asset_scripts()
        )?;
        write!(f, ", {}: {}", "withdrawals", self.withdrawals())?;
        write!(f, ", {}: {}", "bad_block_hashes", self.bad_block_hashes())?;
        write!(f, ", {}: {}", "submit_tx_hash", self.submit_tx_hash())?;
        let extra_count = self.count_extra_fields();
        if extra_count != 0 {
            write!(f, ", .. ({} fields)", extra_count)?;
        }
        write!(f, " }}")
    }
}
impl ::core::default::Default for ExportedBlock {
    fn default() -> Self {
        let v: Vec<u8> = vec![
            0, 0, 90, 2, 0, 0, 100, 1, 0, 0, 28, 0, 0, 0, 80, 1, 0, 0, 84, 1, 0, 0, 88, 1, 0, 0,
            92, 1, 0, 0, 96, 1, 0, 0, 52, 1, 0, 0, 44, 0, 0, 0, 52, 0, 0, 0, 56, 0, 0, 0, 88, 0, 0,
            0, 120, 0, 0, 0, 128, 0, 0, 0, 164, 0, 0, 0, 200, 0, 0, 0, 204, 0, 0, 0, 240, 0, 0, 0,
            0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0,
            0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0,
            0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0,
            0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 4, 0, 0, 0, 0, 0, 0, 0, 4, 0, 0, 0, 0, 0, 0, 0, 0, 0,
            0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0,
            0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0,
            0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0,
            0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0,
            0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0,
            0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0,
            0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 4, 0, 0, 0, 4, 0, 0,
            0, 4, 0, 0, 0,
        ];
        ExportedBlock::new_unchecked(v.into())
    }
}
impl ExportedBlock {
    pub const FIELD_COUNT: usize = 7;
    pub fn total_size(&self) -> usize {
        molecule::unpack_number(self.as_slice()) as usize
    }
    pub fn field_count(&self) -> usize {
        if self.total_size() == molecule::NUMBER_SIZE {
            0
        } else {
            (molecule::unpack_number(&self.as_slice()[molecule::NUMBER_SIZE..]) as usize / 4) - 1
        }
    }
    pub fn count_extra_fields(&self) -> usize {
        self.field_count() - Self::FIELD_COUNT
    }
    pub fn has_extra_fields(&self) -> bool {
        Self::FIELD_COUNT != self.field_count()
    }
    pub fn block(&self) -> L2Block {
        let slice = self.as_slice();
        let start = molecule::unpack_number(&slice[4..]) as usize;
        let end = molecule::unpack_number(&slice[8..]) as usize;
        L2Block::new_unchecked(self.0.slice(start..end))
    }
    pub fn post_global_state(&self) -> GlobalState {
        let slice = self.as_slice();
        let start = molecule::unpack_number(&slice[8..]) as usize;
        let end = molecule::unpack_number(&slice[12..]) as usize;
        GlobalState::new_unchecked(self.0.slice(start..end))
    }
    pub fn deposit_info_vec(&self) -> DepositInfoVec {
        let slice = self.as_slice();
        let start = molecule::unpack_number(&slice[12..]) as usize;
        let end = molecule::unpack_number(&slice[16..]) as usize;
        DepositInfoVec::new_unchecked(self.0.slice(start..end))
    }
    pub fn deposit_asset_scripts(&self) -> ScriptVec {
        let slice = self.as_slice();
        let start = molecule::unpack_number(&slice[16..]) as usize;
        let end = molecule::unpack_number(&slice[20..]) as usize;
        ScriptVec::new_unchecked(self.0.slice(start..end))
    }
    pub fn withdrawals(&self) -> WithdrawalRequestExtraVec {
        let slice = self.as_slice();
        let start = molecule::unpack_number(&slice[20..]) as usize;
        let end = molecule::unpack_number(&slice[24..]) as usize;
        WithdrawalRequestExtraVec::new_unchecked(self.0.slice(start..end))
    }
    pub fn bad_block_hashes(&self) -> Byte32VecVecOpt {
        let slice = self.as_slice();
        let start = molecule::unpack_number(&slice[24..]) as usize;
        let end = molecule::unpack_number(&slice[28..]) as usize;
        Byte32VecVecOpt::new_unchecked(self.0.slice(start..end))
    }
    pub fn submit_tx_hash(&self) -> Byte32Opt {
        let slice = self.as_slice();
        let start = molecule::unpack_number(&slice[28..]) as usize;
        if self.has_extra_fields() {
            let end = molecule::unpack_number(&slice[32..]) as usize;
            Byte32Opt::new_unchecked(self.0.slice(start..end))
        } else {
            Byte32Opt::new_unchecked(self.0.slice(start..))
        }
    }
    pub fn as_reader<'r>(&'r self) -> ExportedBlockReader<'r> {
        ExportedBlockReader::new_unchecked(self.as_slice())
    }
}
impl molecule::prelude::Entity for ExportedBlock {
    type Builder = ExportedBlockBuilder;
    const NAME: &'static str = "ExportedBlock";
    fn new_unchecked(data: molecule::bytes::Bytes) -> Self {
        ExportedBlock(data)
    }
    fn as_bytes(&self) -> molecule::bytes::Bytes {
        self.0.clone()
    }
    fn as_slice(&self) -> &[u8] {
        &self.0[..]
    }
    fn from_slice(slice: &[u8]) -> molecule::error::VerificationResult<Self> {
        ExportedBlockReader::from_slice(slice).map(|reader| reader.to_entity())
    }
    fn from_compatible_slice(slice: &[u8]) -> molecule::error::VerificationResult<Self> {
        ExportedBlockReader::from_compatible_slice(slice).map(|reader| reader.to_entity())
    }
    fn new_builder() -> Self::Builder {
        ::core::default::Default::default()
    }
    fn as_builder(self) -> Self::Builder {
        Self::new_builder()
            .block(self.block())
            .post_global_state(self.post_global_state())
            .deposit_info_vec(self.deposit_info_vec())
            .deposit_asset_scripts(self.deposit_asset_scripts())
            .withdrawals(self.withdrawals())
            .bad_block_hashes(self.bad_block_hashes())
            .submit_tx_hash(self.submit_tx_hash())
    }
}
#[derive(Clone, Copy)]
pub struct ExportedBlockReader<'r>(&'r [u8]);
impl<'r> ::core::fmt::LowerHex for ExportedBlockReader<'r> {
    fn fmt(&self, f: &mut ::core::fmt::Formatter) -> ::core::fmt::Result {
        use molecule::hex_string;
        if f.alternate() {
            write!(f, "0x")?;
        }
        write!(f, "{}", hex_string(self.as_slice()))
    }
}
impl<'r> ::core::fmt::Debug for ExportedBlockReader<'r> {
    fn fmt(&self, f: &mut ::core::fmt::Formatter) -> ::core::fmt::Result {
        write!(f, "{}({:#x})", Self::NAME, self)
    }
}
impl<'r> ::core::fmt::Display for ExportedBlockReader<'r> {
    fn fmt(&self, f: &mut ::core::fmt::Formatter) -> ::core::fmt::Result {
        write!(f, "{} {{ ", Self::NAME)?;
        write!(f, "{}: {}", "block", self.block())?;
        write!(f, ", {}: {}", "post_global_state", self.post_global_state())?;
        write!(f, ", {}: {}", "deposit_info_vec", self.deposit_info_vec())?;
        write!(
            f,
            ", {}: {}",
            "deposit_asset_scripts",
            self.deposit_asset_scripts()
        )?;
        write!(f, ", {}: {}", "withdrawals", self.withdrawals())?;
        write!(f, ", {}: {}", "bad_block_hashes", self.bad_block_hashes())?;
        write!(f, ", {}: {}", "submit_tx_hash", self.submit_tx_hash())?;
        let extra_count = self.count_extra_fields();
        if extra_count != 0 {
            write!(f, ", .. ({} fields)", extra_count)?;
        }
        write!(f, " }}")
    }
}
impl<'r> ExportedBlockReader<'r> {
    pub const FIELD_COUNT: usize = 7;
    pub fn total_size(&self) -> usize {
        molecule::unpack_number(self.as_slice()) as usize
    }
    pub fn field_count(&self) -> usize {
        if self.total_size() == molecule::NUMBER_SIZE {
            0
        } else {
            (molecule::unpack_number(&self.as_slice()[molecule::NUMBER_SIZE..]) as usize / 4) - 1
        }
    }
    pub fn count_extra_fields(&self) -> usize {
        self.field_count() - Self::FIELD_COUNT
    }
    pub fn has_extra_fields(&self) -> bool {
        Self::FIELD_COUNT != self.field_count()
    }
    pub fn block(&self) -> L2BlockReader<'r> {
        let slice = self.as_slice();
        let start = molecule::unpack_number(&slice[4..]) as usize;
        let end = molecule::unpack_number(&slice[8..]) as usize;
        L2BlockReader::new_unchecked(&self.as_slice()[start..end])
    }
    pub fn post_global_state(&self) -> GlobalStateReader<'r> {
        let slice = self.as_slice();
        let start = molecule::unpack_number(&slice[8..]) as usize;
        let end = molecule::unpack_number(&slice[12..]) as usize;
        GlobalStateReader::new_unchecked(&self.as_slice()[start..end])
    }
    pub fn deposit_info_vec(&self) -> DepositInfoVecReader<'r> {
        let slice = self.as_slice();
        let start = molecule::unpack_number(&slice[12..]) as usize;
        let end = molecule::unpack_number(&slice[16..]) as usize;
        DepositInfoVecReader::new_unchecked(&self.as_slice()[start..end])
    }
    pub fn deposit_asset_scripts(&self) -> ScriptVecReader<'r> {
        let slice = self.as_slice();
        let start = molecule::unpack_number(&slice[16..]) as usize;
        let end = molecule::unpack_number(&slice[20..]) as usize;
        ScriptVecReader::new_unchecked(&self.as_slice()[start..end])
    }
    pub fn withdrawals(&self) -> WithdrawalRequestExtraVecReader<'r> {
        let slice = self.as_slice();
        let start = molecule::unpack_number(&slice[20..]) as usize;
        let end = molecule::unpack_number(&slice[24..]) as usize;
        WithdrawalRequestExtraVecReader::new_unchecked(&self.as_slice()[start..end])
    }
    pub fn bad_block_hashes(&self) -> Byte32VecVecOptReader<'r> {
        let slice = self.as_slice();
        let start = molecule::unpack_number(&slice[24..]) as usize;
        let end = molecule::unpack_number(&slice[28..]) as usize;
        Byte32VecVecOptReader::new_unchecked(&self.as_slice()[start..end])
    }
    pub fn submit_tx_hash(&self) -> Byte32OptReader<'r> {
        let slice = self.as_slice();
        let start = molecule::unpack_number(&slice[28..]) as usize;
        if self.has_extra_fields() {
            let end = molecule::unpack_number(&slice[32..]) as usize;
            Byte32OptReader::new_unchecked(&self.as_slice()[start..end])
        } else {
            Byte32OptReader::new_unchecked(&self.as_slice()[start..])
        }
    }
}
impl<'r> molecule::prelude::Reader<'r> for ExportedBlockReader<'r> {
    type Entity = ExportedBlock;
    const NAME: &'static str = "ExportedBlockReader";
    fn to_entity(&self) -> Self::Entity {
        Self::Entity::new_unchecked(self.as_slice().to_owned().into())
    }
    fn new_unchecked(slice: &'r [u8]) -> Self {
        ExportedBlockReader(slice)
    }
    fn as_slice(&self) -> &'r [u8] {
        self.0
    }
    fn verify(slice: &[u8], compatible: bool) -> molecule::error::VerificationResult<()> {
        use molecule::verification_error as ve;
        let slice_len = slice.len();
        if slice_len < molecule::NUMBER_SIZE {
            return ve!(Self, HeaderIsBroken, molecule::NUMBER_SIZE, slice_len);
        }
        let total_size = molecule::unpack_number(slice) as usize;
        if slice_len != total_size {
            return ve!(Self, TotalSizeNotMatch, total_size, slice_len);
        }
        if slice_len == molecule::NUMBER_SIZE && Self::FIELD_COUNT == 0 {
            return Ok(());
        }
        if slice_len < molecule::NUMBER_SIZE * 2 {
            return ve!(Self, HeaderIsBroken, molecule::NUMBER_SIZE * 2, slice_len);
        }
        let offset_first = molecule::unpack_number(&slice[molecule::NUMBER_SIZE..]) as usize;
        if offset_first % molecule::NUMBER_SIZE != 0 || offset_first < molecule::NUMBER_SIZE * 2 {
            return ve!(Self, OffsetsNotMatch);
        }
        if slice_len < offset_first {
            return ve!(Self, HeaderIsBroken, offset_first, slice_len);
        }
        let field_count = offset_first / molecule::NUMBER_SIZE - 1;
        if field_count < Self::FIELD_COUNT {
            return ve!(Self, FieldCountNotMatch, Self::FIELD_COUNT, field_count);
        } else if !compatible && field_count > Self::FIELD_COUNT {
            return ve!(Self, FieldCountNotMatch, Self::FIELD_COUNT, field_count);
        };
        let mut offsets: Vec<usize> = slice[molecule::NUMBER_SIZE..offset_first]
            .chunks_exact(molecule::NUMBER_SIZE)
            .map(|x| molecule::unpack_number(x) as usize)
            .collect();
        offsets.push(total_size);
        if offsets.windows(2).any(|i| i[0] > i[1]) {
            return ve!(Self, OffsetsNotMatch);
        }
        L2BlockReader::verify(&slice[offsets[0]..offsets[1]], compatible)?;
        GlobalStateReader::verify(&slice[offsets[1]..offsets[2]], compatible)?;
        DepositInfoVecReader::verify(&slice[offsets[2]..offsets[3]], compatible)?;
        ScriptVecReader::verify(&slice[offsets[3]..offsets[4]], compatible)?;
        WithdrawalRequestExtraVecReader::verify(&slice[offsets[4]..offsets[5]], compatible)?;
        Byte32VecVecOptReader::verify(&slice[offsets[5]..offsets[6]], compatible)?;
        Byte32OptReader::verify(&slice[offsets[6]..offsets[7]], compatible)?;
        Ok(())
    }
}
#[derive(Debug, Default)]
pub struct ExportedBlockBuilder {
    pub(crate) block: L2Block,
    pub(crate) post_global_state: GlobalState,
    pub(crate) deposit_info_vec: DepositInfoVec,
    pub(crate) deposit_asset_scripts: ScriptVec,
    pub(crate) withdrawals: WithdrawalRequestExtraVec,
    pub(crate) bad_block_hashes: Byte32VecVecOpt,
    pub(crate) submit_tx_hash: Byte32Opt,
}
impl ExportedBlockBuilder {
    pub const FIELD_COUNT: usize = 7;
    pub fn block(mut self, v: L2Block) -> Self {
        self.block = v;
        self
    }
    pub fn post_global_state(mut self, v: GlobalState) -> Self {
        self.post_global_state = v;
        self
    }
    pub fn deposit_info_vec(mut self, v: DepositInfoVec) -> Self {
        self.deposit_info_vec = v;
        self
    }
    pub fn deposit_asset_scripts(mut self, v: ScriptVec) -> Self {
        self.deposit_asset_scripts = v;
        self
    }
    pub fn withdrawals(mut self, v: WithdrawalRequestExtraVec) -> Self {
        self.withdrawals = v;
        self
    }
    pub fn bad_block_hashes(mut self, v: Byte32VecVecOpt) -> Self {
        self.bad_block_hashes = v;
        self
    }
    pub fn submit_tx_hash(mut self, v: Byte32Opt) -> Self {
        self.submit_tx_hash = v;
        self
    }
}
impl molecule::prelude::Builder for ExportedBlockBuilder {
    type Entity = ExportedBlock;
    const NAME: &'static str = "ExportedBlockBuilder";
    fn expected_length(&self) -> usize {
        molecule::NUMBER_SIZE * (Self::FIELD_COUNT + 1)
            + self.block.as_slice().len()
            + self.post_global_state.as_slice().len()
            + self.deposit_info_vec.as_slice().len()
            + self.deposit_asset_scripts.as_slice().len()
            + self.withdrawals.as_slice().len()
            + self.bad_block_hashes.as_slice().len()
            + self.submit_tx_hash.as_slice().len()
    }
    fn write<W: molecule::io::Write>(&self, writer: &mut W) -> molecule::io::Result<()> {
        let mut total_size = molecule::NUMBER_SIZE * (Self::FIELD_COUNT + 1);
        let mut offsets = Vec::with_capacity(Self::FIELD_COUNT);
        offsets.push(total_size);
        total_size += self.block.as_slice().len();
        offsets.push(total_size);
        total_size += self.post_global_state.as_slice().len();
        offsets.push(total_size);
        total_size += self.deposit_info_vec.as_slice().len();
        offsets.push(total_size);
        total_size += self.deposit_asset_scripts.as_slice().len();
        offsets.push(total_size);
        total_size += self.withdrawals.as_slice().len();
        offsets.push(total_size);
        total_size += self.bad_block_hashes.as_slice().len();
        offsets.push(total_size);
        total_size += self.submit_tx_hash.as_slice().len();
        writer.write_all(&molecule::pack_number(total_size as molecule::Number))?;
        for offset in offsets.into_iter() {
            writer.write_all(&molecule::pack_number(offset as molecule::Number))?;
        }
        writer.write_all(self.block.as_slice())?;
        writer.write_all(self.post_global_state.as_slice())?;
        writer.write_all(self.deposit_info_vec.as_slice())?;
        writer.write_all(self.deposit_asset_scripts.as_slice())?;
        writer.write_all(self.withdrawals.as_slice())?;
        writer.write_all(self.bad_block_hashes.as_slice())?;
        writer.write_all(self.submit_tx_hash.as_slice())?;
        Ok(())
    }
    fn build(&self) -> Self::Entity {
        let mut inner = Vec::with_capacity(self.expected_length());
        self.write(&mut inner)
            .unwrap_or_else(|_| panic!("{} build should be ok", Self::NAME));
        ExportedBlock::new_unchecked(inner.into())
    }
}
