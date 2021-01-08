use gw_common::H256;
use gw_types::{
    bytes::Bytes,
    packed::{self, StartChallenge, StartChallengeWitness},
    prelude::*,
};
use std::{
    collections::HashMap,
    fmt::{self, Display},
};

#[derive(Debug, PartialEq, Clone, Eq, Default)]
pub struct LogItem {
    pub account_id: u32,
    pub data: Vec<u8>,
}

impl From<LogItem> for packed::LogItem {
    fn from(item: LogItem) -> packed::LogItem {
        let LogItem { account_id, data } = item;
        packed::LogItem::new_builder()
            .account_id(account_id.pack())
            .data(Bytes::from(data).pack())
            .build()
    }
}

impl From<packed::LogItem> for LogItem {
    fn from(data: packed::LogItem) -> LogItem {
        let account_id: u32 = data.account_id().unpack();
        let data: Bytes = data.data().unpack();
        LogItem {
            account_id: account_id,
            data: data.as_ref().to_vec(),
        }
    }
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
    // account id -> log data
    pub logs: Vec<LogItem>,
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
