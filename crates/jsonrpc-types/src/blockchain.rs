use anyhow::{anyhow, Error as JsonError};
use ckb_fixed_hash::H256;
use ckb_jsonrpc_types::{JsonBytes, Uint32, Uint64};
use gw_types::{packed, prelude::*};
use serde::{Deserialize, Serialize};
use std::convert::{TryFrom, TryInto};

#[derive(Clone, Serialize, Deserialize, PartialEq, Eq, Hash, Debug)]
#[serde(rename_all = "snake_case")]
pub enum ScriptHashType {
    /// Type "data" matches script code via cell data hash.
    Data,
    /// Type "type" matches script code via cell type script hash.
    Type,
    /// Type "data" matches script code via cell data hash, and run the script code in v1 CKB VM.
    Data1,
}

impl Default for ScriptHashType {
    fn default() -> Self {
        ScriptHashType::Data
    }
}

impl From<ScriptHashType> for packed::Byte {
    fn from(json: ScriptHashType) -> packed::Byte {
        match json {
            ScriptHashType::Data => packed::Byte::new(0),
            ScriptHashType::Type => packed::Byte::new(1),
            ScriptHashType::Data1 => packed::Byte::new(2),
        }
    }
}

impl From<ckb_jsonrpc_types::ScriptHashType> for ScriptHashType {
    fn from(hash_type: ckb_jsonrpc_types::ScriptHashType) -> ScriptHashType {
        match hash_type {
            ckb_jsonrpc_types::ScriptHashType::Data => ScriptHashType::Data,
            ckb_jsonrpc_types::ScriptHashType::Type => ScriptHashType::Type,
            ckb_jsonrpc_types::ScriptHashType::Data1 => ScriptHashType::Data1,
        }
    }
}

impl From<ScriptHashType> for ckb_jsonrpc_types::ScriptHashType {
    fn from(hash_type: ScriptHashType) -> ckb_jsonrpc_types::ScriptHashType {
        match hash_type {
            ScriptHashType::Data => ckb_jsonrpc_types::ScriptHashType::Data,
            ScriptHashType::Type => ckb_jsonrpc_types::ScriptHashType::Type,
            ScriptHashType::Data1 => ckb_jsonrpc_types::ScriptHashType::Data1,
        }
    }
}

impl TryFrom<packed::Byte> for ScriptHashType {
    type Error = JsonError;

    fn try_from(v: packed::Byte) -> Result<ScriptHashType, Self::Error> {
        match u8::from(v) {
            0 => Ok(ScriptHashType::Data),
            1 => Ok(ScriptHashType::Type),
            2 => Ok(ScriptHashType::Data1),
            _ => Err(anyhow!("Invalid script hash type {}", v)),
        }
    }
}

#[derive(Clone, Default, Serialize, Deserialize, PartialEq, Eq, Hash, Debug)]
#[serde(deny_unknown_fields)]
pub struct Script {
    /// The hash used to match the script code.
    pub code_hash: H256,
    /// Specifies how to use the `code_hash` to match the script code.
    pub hash_type: ScriptHashType,
    /// Arguments for script.
    pub args: JsonBytes,
}

impl Script {
    pub fn hash(&self) -> H256 {
        let script: packed::Script = self.to_owned().into();
        H256(script.hash())
    }
}

impl From<Script> for packed::Script {
    fn from(json: Script) -> Self {
        let Script {
            args,
            code_hash,
            hash_type,
        } = json;
        packed::Script::new_builder()
            .args(args.into_bytes().pack())
            .code_hash(code_hash.pack())
            .hash_type(hash_type.into())
            .build()
    }
}

impl From<packed::Script> for Script {
    fn from(input: packed::Script) -> Script {
        Script {
            code_hash: input.code_hash().unpack(),
            args: JsonBytes::from_bytes(input.args().unpack()),
            hash_type: ScriptHashType::try_from(input.hash_type()).expect("checked data"),
        }
    }
}

impl From<ckb_jsonrpc_types::Script> for Script {
    fn from(script: ckb_jsonrpc_types::Script) -> Script {
        Script {
            code_hash: script.code_hash,
            hash_type: script.hash_type.into(),
            args: script.args,
        }
    }
}

impl From<Script> for ckb_jsonrpc_types::Script {
    fn from(script: Script) -> ckb_jsonrpc_types::Script {
        ckb_jsonrpc_types::Script {
            code_hash: script.code_hash,
            hash_type: script.hash_type.into(),
            args: script.args,
        }
    }
}

#[derive(Clone, Default, Serialize, Deserialize, PartialEq, Eq, Hash, Debug)]
#[serde(deny_unknown_fields)]
pub struct Transaction {
    /// Reserved for future usage. It must equal 0 in current version.
    pub version: Version,
    /// An array of cell deps.
    ///
    /// CKB locates lock script and type script code via cell deps. The script also can uses syscalls
    /// to read the cells here.
    ///
    /// Unlike inputs, the live cells can be used as cell deps in multiple transactions.
    pub cell_deps: Vec<CellDep>,
    /// An array of header deps.
    ///
    /// The block must already be in the canonical chain.
    ///
    /// Lock script and type script can read the header information of blocks listed here.
    pub header_deps: Vec<H256>,
    /// An array of input cells.
    ///
    /// In the canonical chain, any cell can only appear as an input once.
    pub inputs: Vec<CellInput>,
    /// An array of output cells.
    pub outputs: Vec<CellOutput>,
    /// Output cells data.
    ///
    /// This is a parallel array of outputs. The cell capacity, lock, and type of the output i is
    /// `outputs[i]` and its data is `outputs_data[i]`.
    pub outputs_data: Vec<JsonBytes>,
    /// An array of variable-length binaries.
    ///
    /// Lock script and type script can read data here to verify the transaction.
    ///
    /// For example, the bundled secp256k1 lock script requires storing the signature in
    /// `witnesses`.
    pub witnesses: Vec<JsonBytes>,
}

pub type Version = Uint32;

#[derive(Clone, Default, Serialize, Deserialize, PartialEq, Eq, Hash, Debug)]
#[serde(deny_unknown_fields)]
pub struct CellDep {
    /// Dependency type.
    pub dep_type: DepType,
    /// Reference to the cell.
    pub out_point: OutPoint,
}

impl From<CellDep> for packed::CellDep {
    fn from(json: CellDep) -> Self {
        let CellDep {
            dep_type,
            out_point,
        } = json;
        let dep_type: packed::Byte = dep_type.into();
        packed::CellDep::new_builder()
            .dep_type(dep_type)
            .out_point(out_point.into())
            .build()
    }
}

impl From<packed::CellDep> for CellDep {
    fn from(data: packed::CellDep) -> CellDep {
        CellDep {
            dep_type: data.dep_type().try_into().expect("dep type"),
            out_point: data.out_point().into(),
        }
    }
}

impl From<ckb_jsonrpc_types::CellDep> for CellDep {
    fn from(cell_dep: ckb_jsonrpc_types::CellDep) -> CellDep {
        CellDep {
            dep_type: cell_dep.dep_type.into(),
            out_point: cell_dep.out_point.into(),
        }
    }
}

#[derive(Clone, Default, Serialize, Deserialize, PartialEq, Eq, Hash, Debug)]
#[serde(deny_unknown_fields)]
pub struct CellInput {
    /// Restrict when the transaction can be committed into the chain.
    ///
    /// See the RFC [Transaction valid since](https://github.com/nervosnetwork/rfcs/blob/master/rfcs/0017-tx-valid-since/0017-tx-valid-since.md).
    pub since: Uint64,
    /// Reference to the input cell.
    pub previous_output: OutPoint,
}

#[derive(Clone, Default, Serialize, Deserialize, PartialEq, Eq, Hash, Debug)]
#[serde(deny_unknown_fields)]
pub struct CellOutput {
    /// The cell capacity.
    ///
    /// The capacity of a cell is the value of the cell in Shannons. It is also the upper limit of
    /// the cell occupied storage size where every 100,000,000 Shannons give 1-byte storage.
    pub capacity: Capacity,
    /// The lock script.
    pub lock: Script,
    /// The optional type script.
    ///
    /// The JSON field name is "type".
    #[serde(rename = "type")]
    pub type_: Option<Script>,
}

#[derive(Clone, Default, Serialize, Deserialize, PartialEq, Eq, Hash, Debug)]
#[serde(deny_unknown_fields)]
pub struct OutPoint {
    /// Transaction hash in which the cell is an output.
    pub tx_hash: H256,
    /// The output index of the cell in the transaction specified by `tx_hash`.
    pub index: Uint32,
}

impl From<OutPoint> for packed::OutPoint {
    fn from(json: OutPoint) -> Self {
        let OutPoint { tx_hash, index } = json;
        let index: u32 = index.into();
        packed::OutPoint::new_builder()
            .tx_hash(tx_hash.pack())
            .index(index.pack())
            .build()
    }
}

impl From<packed::OutPoint> for OutPoint {
    fn from(data: packed::OutPoint) -> OutPoint {
        let index: u32 = data.index().unpack();
        OutPoint {
            tx_hash: data.tx_hash().unpack(),
            index: index.into(),
        }
    }
}

impl From<ckb_jsonrpc_types::OutPoint> for OutPoint {
    fn from(out_point: ckb_jsonrpc_types::OutPoint) -> OutPoint {
        OutPoint {
            tx_hash: out_point.tx_hash,
            index: out_point.index,
        }
    }
}

#[derive(Clone, Serialize, Deserialize, PartialEq, Eq, Hash, Debug)]
#[serde(rename_all = "snake_case")]
pub enum DepType {
    /// Type "code".
    ///
    /// Use the cell itself as the dep cell.
    Code,
    /// Type "dep_group".
    ///
    /// The cell is a dep group which members are cells. These members are used as dep cells
    /// instead of the group itself.
    ///
    /// The dep group stores the array of `OutPoint`s serialized via molecule in the cell data.
    /// Each `OutPoint` points to one cell member.
    DepGroup,
}

impl From<DepType> for packed::Byte {
    fn from(json: DepType) -> Self {
        match json {
            DepType::Code => gw_types::core::DepType::Code.into(),
            DepType::DepGroup => gw_types::core::DepType::DepGroup.into(),
        }
    }
}

impl From<packed::Byte> for DepType {
    fn from(data: packed::Byte) -> DepType {
        let dep_type: gw_types::core::DepType = data.try_into().expect("dep type");
        match dep_type {
            gw_types::core::DepType::Code => DepType::Code,
            gw_types::core::DepType::DepGroup => DepType::DepGroup,
        }
    }
}

impl From<ckb_jsonrpc_types::DepType> for DepType {
    fn from(dep_type: ckb_jsonrpc_types::DepType) -> Self {
        match dep_type {
            ckb_jsonrpc_types::DepType::Code => DepType::Code,
            ckb_jsonrpc_types::DepType::DepGroup => DepType::DepGroup,
        }
    }
}

impl Default for DepType {
    fn default() -> Self {
        DepType::Code
    }
}

pub type Capacity = Uint64;

#[derive(Clone, Serialize, Deserialize, PartialEq, Eq, Hash, Debug, Default)]
#[serde(rename_all = "snake_case")]
pub struct NumberHash {
    block_number: Uint64,
    block_hash: H256,
}

impl From<NumberHash> for packed::NumberHash {
    fn from(v: NumberHash) -> Self {
        let NumberHash {
            block_number,
            block_hash,
        } = v;
        packed::NumberHash::new_builder()
            .number(block_number.value().pack())
            .block_hash(block_hash.pack())
            .build()
    }
}

impl From<packed::NumberHash> for NumberHash {
    fn from(v: packed::NumberHash) -> NumberHash {
        let number: u64 = v.number().unpack();
        let block_hash = v.block_hash().unpack();
        Self {
            block_number: number.into(),
            block_hash,
        }
    }
}
