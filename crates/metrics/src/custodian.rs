use gw_common::{CKB_SUDT_SCRIPT_ARGS, H256};
use gw_store::{traits::chain_store::ChainStore, Store};
use gw_telemetry::metric::{
    encoding::text::Encode, family::Family, gauge::Gauge, prometheus_client, registry::Registry,
    Lazy,
};
use gw_types::prelude::Unpack;
use smol_str::SmolStr;

static CUSTODIAN_METRICS: Lazy<CustodianMetrics> = Lazy::new(CustodianMetrics::default);

pub fn custodian() -> &'static CustodianMetrics {
    &CUSTODIAN_METRICS
}

#[derive(Default)]
pub struct CustodianMetrics {
    finalized_custodians: Family<CustodianLabel, Gauge>,
}

impl CustodianMetrics {
    pub(crate) fn register(&self, config: &crate::Config, registry: &mut Registry) {
        if config.node_mode != gw_config::NodeMode::FullNode {
            return;
        }

        registry.register(
            "finalized_custodians",
            "Finalized custodians",
            Box::new(self.finalized_custodians.clone()),
        );
    }

    pub fn finalized_custodians(&self, store: &Store) {
        let config = crate::config();
        if !config.custodian_enabled {
            return;
        }

        let get_local_finalized = { store.get_last_valid_tip_block_hash().ok() }
            .and_then(|bh| store.get_block_number(&bh).ok().flatten())
            .map(|bn| store.get_block_post_finalized_custodian_capacity(bn));

        let local = match get_local_finalized.flatten() {
            Some(local) => local.as_reader().unpack(),
            None => return,
        };

        let cal = |balance: &u128, decimal| balance.saturating_div(10u128.pow(decimal)) as u64;
        for custodian in config.custodian_map.values() {
            if custodian.type_hash == H256::from(CKB_SUDT_SCRIPT_ARGS) {
                self.finalized(custodian, |g, d| g.set(cal(&(local.capacity as u128), d)));
                continue;
            }
            if let Some((balance, _)) = local.sudt.get::<[u8; 32]>(&custodian.type_hash.into()) {
                self.finalized(custodian, |g, d| g.set(cal(balance, d)));
            }
        }
    }

    fn finalized<F, O>(&self, custodian: &crate::Custodian, f: F) -> O
    where
        F: Fn(&Gauge, u32) -> O,
    {
        let gauge = self.finalized_custodians.get_or_create(&CustodianLabel {
            symbol: EncodableSmolStr(custodian.symbol.clone()),
        });

        f(&gauge, custodian.decimal)
    }
}

#[derive(Debug, Clone, Hash, PartialEq, Eq, Encode)]
struct CustodianLabel {
    pub symbol: EncodableSmolStr,
}

#[derive(Debug, Clone, Hash, PartialEq, Eq)]
struct EncodableSmolStr(SmolStr);

impl Encode for EncodableSmolStr {
    fn encode(&self, writer: &mut dyn std::io::Write) -> Result<(), std::io::Error> {
        self.0.as_str().encode(writer)
    }
}
