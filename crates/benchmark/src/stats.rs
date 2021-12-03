use anyhow::Result;
use std::{
    collections::{HashMap, VecDeque},
    time::{Duration, Instant},
};
use tokio::sync::{mpsc, oneshot};

use crate::tx::TxStatus;

struct StatsActor {
    receiver: mpsc::Receiver<StatsReqMsg>,
    api_stats_handler_map: HashMap<String, ApiStatsHandler>,
    limit: usize,
    tx_stats: TxStats,
}

impl StatsActor {
    fn new(limit: usize, receiver: mpsc::Receiver<StatsReqMsg>) -> Self {
        Self {
            receiver,
            limit,
            api_stats_handler_map: HashMap::new(),
            tx_stats: TxStats {
                ts: Instant::now(),
                pending_commit: 0,
                committed: 0,
                timeout: 0,
                failure: 0,
                pending_commit_tps: 0f32,
                committed_tps: 0f32,
            },
        }
    }

    fn handle(&mut self, msg: StatsReqMsg) {
        match msg {
            StatsReqMsg::SendApiStatus {
                api,
                duration,
                status,
            } => self.handle_api_status(api, duration, status),
            StatsReqMsg::SendTxStatus(status) => self.handle_tx_status(status),
            StatsReqMsg::Get(sender) => self.handle_get_stats(sender),
        }
    }

    fn handle_api_status(&mut self, api: String, duration: Duration, status: ApiStatus) {
        if !self.api_stats_handler_map.contains_key(&api) {
            let handler = ApiStatsHandler::new(self.limit);
            let _ = self.api_stats_handler_map.insert(api.clone(), handler);
        }
        if let Some(handler) = self.api_stats_handler_map.get_mut(&api) {
            handler.insert(status, duration);
        }
    }

    fn handle_tx_status(&mut self, status: TxStatus) {
        match status {
            TxStatus::PendingCommit => self.tx_stats.pending_commit += 1,
            TxStatus::Committed => self.tx_stats.committed += 1,
            TxStatus::Failure => self.tx_stats.failure += 1,
            TxStatus::Timeout => self.tx_stats.timeout += 1,
        };
        let dur = self.tx_stats.ts.elapsed().as_secs() as f32;
        let pending_commit_tps = self.tx_stats.pending_commit as f32 / dur;
        let committed_tps = self.tx_stats.committed as f32 / dur;
        self.tx_stats.pending_commit_tps = pending_commit_tps;
        self.tx_stats.committed_tps = committed_tps;
    }

    fn handle_get_stats(&self, sender: oneshot::Sender<Stats>) {
        let api = self
            .api_stats_handler_map
            .iter()
            .map(|(k, v)| {
                let s = v.stats();
                (k.clone(), s)
            })
            .collect();
        let tx = self.tx_stats.clone();
        let stats = Stats { tx, api };
        let _ = sender.send(stats);
    }
}

async fn stats_handler(mut actor: StatsActor) {
    log::info!("stats handler is running now");
    while let Some(msg) = actor.receiver.recv().await {
        actor.handle(msg);
    }
}

#[derive(Debug)]
pub struct Stats {
    tx: TxStats,
    api: HashMap<String, ApiStats>,
}

#[derive(Debug, Clone)]
pub struct TxStats {
    timeout: usize,
    failure: usize,
    pending_commit: usize,
    committed: usize,
    ts: Instant,
    pending_commit_tps: f32,
    committed_tps: f32,
}

#[derive(Debug)]
pub struct ApiStats {
    max: u128,
    min: u128,
    avg: f32,
    success: usize,
    failure: usize,
}
struct ApiStatsHandler {
    deq: VecDeque<Duration>,
    limit: usize,
    max: u128,
    min: u128,
    success: usize,
    failure: usize,
}

impl ApiStatsHandler {
    fn new(limit: usize) -> Self {
        Self {
            deq: VecDeque::new(),
            limit,
            max: 0,
            min: u128::MAX,
            success: 0,
            failure: 0,
        }
    }

    fn insert(&mut self, status: ApiStatus, duration: Duration) {
        let dur_ms = duration.as_millis();
        if dur_ms > self.max {
            self.max = dur_ms;
        }
        if dur_ms < self.min {
            self.min = dur_ms;
        }
        self.deq.push_back(duration);
        if self.deq.len() > self.limit {
            self.deq.pop_front();
        }
        match status {
            ApiStatus::Success => self.success += 1,
            ApiStatus::Failure => self.failure += 1,
        };
    }

    fn stats(&self) -> ApiStats {
        let mut sum = 0;
        for i in &self.deq {
            let ms = i.as_millis();
            sum += ms;
        }
        let avg = if self.deq.is_empty() {
            0f32
        } else {
            sum as f32 / self.deq.len() as f32
        };
        ApiStats {
            min: self.min,
            max: self.max,
            avg,
            success: self.success,
            failure: self.failure,
        }
    }
}

#[derive(Clone)]
pub struct StatsHandler {
    sender: mpsc::Sender<StatsReqMsg>,
}

impl StatsHandler {
    pub fn new() -> Self {
        let (sender, receiver) = mpsc::channel(200);
        let actor = StatsActor::new(1000, receiver);
        tokio::spawn(stats_handler(actor));
        Self { sender }
    }

    pub async fn send_api_stats(&self, api: String, duration: Duration, status: ApiStatus) {
        let msg = StatsReqMsg::SendApiStatus {
            api,
            duration,
            status,
        };
        let _ = self.sender.send(msg).await;
    }

    pub async fn send_tx_stats(&self, status: TxStatus) {
        let msg = StatsReqMsg::SendTxStatus(status);
        let _ = self.sender.send(msg).await;
    }

    pub async fn get_stats(&self) -> Result<Stats> {
        let (send, recv) = oneshot::channel();
        let msg = StatsReqMsg::Get(send);
        let _ = self.sender.send(msg).await;
        let stats = recv.await?;
        Ok(stats)
    }
}

impl Default for StatsHandler {
    fn default() -> Self {
        Self::new()
    }
}

pub enum ApiStatus {
    Success,
    Failure,
}

pub enum StatsReqMsg {
    SendApiStatus {
        api: String,
        duration: Duration,
        status: ApiStatus,
    },
    SendTxStatus(TxStatus),
    Get(oneshot::Sender<Stats>),
}
