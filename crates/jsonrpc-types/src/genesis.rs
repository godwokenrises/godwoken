use ckb_jsonrpc_types::JsonBytes;
use gw_common::sparse_merkle_tree::{self as smt, tree};
use gw_store::genesis;
use gw_types::{packed, prelude::*, H256};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;

use crate::godwoken::GlobalState;

#[derive(Clone, Serialize, Deserialize, PartialEq, Eq, Hash, Debug, Default)]
#[serde(rename_all = "snake_case")]
pub struct BranchNode {
    pub fork_height: u8,
    pub key: H256,
    pub node: H256,
    pub sibling: H256,
}

impl From<BranchNode> for tree::BranchNode {
    fn from(bn: BranchNode) -> Self {
        let key: [u8; 32] = bn.key.pack().unpack();
        let node: [u8; 32] = bn.node.pack().unpack();
        let sibling: [u8; 32] = bn.sibling.pack().unpack();

        tree::BranchNode {
            fork_height: bn.fork_height,
            key: smt::H256::from(key),
            node: smt::H256::from(node),
            sibling: smt::H256::from(sibling),
        }
    }
}

impl From<tree::BranchNode> for BranchNode {
    fn from(bn: tree::BranchNode) -> Self {
        let key: [u8; 32] = bn.key.into();
        let node: [u8; 32] = bn.node.into();
        let sibling: [u8; 32] = bn.sibling.into();

        BranchNode {
            fork_height: bn.fork_height,
            key: key.pack().unpack(),
            node: node.pack().unpack(),
            sibling: sibling.pack().unpack(),
        }
    }
}

#[derive(Clone, Serialize, Deserialize, PartialEq, Eq, Hash, Debug, Default)]
#[serde(rename_all = "snake_case")]
pub struct LeafNode {
    pub key: H256,
    pub value: H256,
}

impl From<LeafNode> for tree::LeafNode<smt::H256> {
    fn from(ln: LeafNode) -> Self {
        let key: [u8; 32] = ln.key.pack().unpack();
        let value: [u8; 32] = ln.value.pack().unpack();

        tree::LeafNode {
            key: key.into(),
            value: value.into(),
        }
    }
}

impl From<tree::LeafNode<smt::H256>> for LeafNode {
    fn from(ln: tree::LeafNode<smt::H256>) -> Self {
        let key: [u8; 32] = ln.key.into();
        let value: [u8; 32] = ln.value.into();

        LeafNode {
            key: key.pack().unpack(),
            value: value.pack().unpack(),
        }
    }
}

#[derive(Clone, Serialize, Deserialize, PartialEq, Eq, Hash, Debug, Default)]
#[serde(rename_all = "snake_case")]
pub struct BranchMapEntry {
    pub key: H256,
    pub value: BranchNode,
}

#[derive(Clone, Serialize, Deserialize, PartialEq, Eq, Hash, Debug, Default)]
#[serde(rename_all = "snake_case")]
pub struct LeafMapEntry {
    pub key: H256,
    pub value: LeafNode,
}

#[derive(Clone, Serialize, Deserialize, PartialEq, Eq, Hash, Debug, Default)]
#[serde(rename_all = "snake_case")]
pub struct GenesisWithSMTState {
    pub genesis: JsonBytes,
    pub branches_map: Vec<BranchMapEntry>,
    pub leaves_map: Vec<LeafMapEntry>,
    pub global_state: GlobalState,
}

impl From<GenesisWithSMTState> for genesis::GenesisWithSMTState {
    fn from(genesis: GenesisWithSMTState) -> Self {
        let branches_map = {
            let mut m = HashMap::default();
            for entry in genesis.branches_map {
                let key: [u8; 32] = entry.key.pack().unpack();
                m.insert(smt::H256::from(key), entry.value.into());
            }
            m
        };
        let leaves_map = {
            let mut m = HashMap::default();
            for entry in genesis.leaves_map {
                let key: [u8; 32] = entry.key.pack().unpack();
                m.insert(smt::H256::from(key), entry.value.into());
            }
            m
        };
        genesis::GenesisWithSMTState {
            genesis: packed::L2Block::from_slice(genesis.genesis.into_bytes().as_ref())
                .expect("Build packed::L2Block from slice"),
            branches_map,
            leaves_map,
            global_state: genesis.global_state.into(),
        }
    }
}

impl From<genesis::GenesisWithSMTState> for GenesisWithSMTState {
    fn from(genesis: genesis::GenesisWithSMTState) -> Self {
        GenesisWithSMTState {
            genesis: JsonBytes::from_bytes(genesis.genesis.as_bytes()),
            global_state: genesis.global_state.into(),
            branches_map: genesis
                .branches_map
                .into_iter()
                .map(|(key, value)| {
                    let key: [u8; 32] = key.into();
                    BranchMapEntry {
                        key: key.pack().unpack(),
                        value: value.into(),
                    }
                })
                .collect(),
            leaves_map: genesis
                .leaves_map
                .into_iter()
                .map(|(key, value)| {
                    let key: [u8; 32] = key.into();
                    LeafMapEntry {
                        key: key.pack().unpack(),
                        value: value.into(),
                    }
                })
                .collect(),
        }
    }
}

#[derive(Clone, Serialize, Deserialize, PartialEq, Eq, Hash, Debug, Default)]
#[serde(rename_all = "snake_case")]
pub struct GenesisSetup {
    pub genesis: GenesisWithSMTState,
    pub header_info: JsonBytes,
}
