use gw_common::H256;
use gw_types::{
    packed::{StartChallenge, StartChallengeWitness},
    prelude::*,
};
use std::{
    collections::HashMap,
    fmt::{self, Display},
};

#[derive(Debug, PartialEq, Clone, Eq, Default)]
pub struct TxReceipt {
    pub tx_witness_hash: H256,
    // hash(account_root|account_count)
    pub compacted_post_account_root: H256,
    pub read_data_hashes: Vec<H256>,
}

#[derive(Debug, PartialEq, Clone, Eq, Default)]
pub struct RunResult {
    pub read_values: HashMap<H256, H256>,
    pub write_values: HashMap<H256, H256>,
    pub return_data: Vec<u8>,
    pub account_count: Option<u32>,
    pub new_scripts: HashMap<H256, Vec<u8>>,
    pub write_data: HashMap<H256, Vec<u8>>,
    // data hash -> data full size
    pub read_data: HashMap<H256, usize>,
}

#[derive(Debug, Clone)]
pub struct ChallengeContext {
    pub args: StartChallenge,
    pub witness: StartChallengeWitness,
}

impl Display for ChallengeContext {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{{args: {}, witness: {}}}", self.args, self.witness)
    }
}

impl PartialEq for ChallengeContext {
    fn eq(&self, other: &Self) -> bool {
        self.args.as_slice() == other.args.as_slice()
            && self.witness.as_slice() == other.witness.as_slice()
    }
}

impl Eq for ChallengeContext {}
