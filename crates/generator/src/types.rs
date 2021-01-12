use gw_common::H256;
use gw_types::packed::{self, StartChallenge, StartChallengeWitness};
use std::{
    collections::HashMap,
    fmt::{self, Display},
};

#[derive(Debug, Clone, Default)]
pub struct RunResult {
    pub read_values: HashMap<H256, H256>,
    pub write_values: HashMap<H256, H256>,
    pub return_data: Vec<u8>,
    pub account_count: Option<u32>,
    pub new_scripts: HashMap<H256, Vec<u8>>,
    pub write_data: HashMap<H256, Vec<u8>>,
    // data hash -> data full size
    pub read_data: HashMap<H256, usize>,
    // log data
    pub logs: Vec<packed::LogItem>,
}

#[derive(Debug, Clone, Eq, PartialEq)]
pub struct ChallengeContext {
    pub args: StartChallenge,
    pub witness: StartChallengeWitness,
}

impl Display for ChallengeContext {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{{args: {}, witness: {}}}", self.args, self.witness)
    }
}
