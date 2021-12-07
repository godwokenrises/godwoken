use rocksdb::ops::{GetColumnFamilys, GetProperty, GetPropertyCF};
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
enum PropertyValue<T> {
    Value(T),
    Null,
    Error(String),
}

impl PropertyValue<u64> {
    #[allow(dead_code)]
    pub(crate) fn as_i64(&self) -> i64 {
        match self {
            Self::Value(v) => *v as i64,
            Self::Null => -1,
            Self::Error(_) => -2,
        }
    }
}

impl<T> From<Result<Option<T>, String>> for PropertyValue<T> {
    fn from(res: Result<Option<T>, String>) -> Self {
        match res {
            Ok(Some(v)) => Self::Value(v),
            Ok(None) => Self::Null,
            Err(e) => Self::Error(e),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CfMemStat {
    name: String,
    type_: String,
    value: PropertyValue<u64>,
}

/// A trait which used to track the RocksDB memory usage.
///
/// References: [Memory usage in RocksDB](https://github.com/facebook/rocksdb/wiki/Memory-usage-in-RocksDB)
pub trait TrackRocksDBMemory {
    /// Gather memory statistics through [ckb-metrics](../../ckb_metrics/index.html)
    fn gather_memory_stats(&self) -> Vec<CfMemStat> {
        let mut stats = Vec::new();
        stats.extend(self.gather_int_values("estimate-table-readers-mem"));
        stats.extend(self.gather_int_values("size-all-mem-tables"));
        stats.extend(self.gather_int_values("cur-size-all-mem-tables"));
        stats.extend(self.gather_int_values("block-cache-capacity"));
        stats.extend(self.gather_int_values("block-cache-usage"));
        stats.extend(self.gather_int_values("block-cache-pinned-usage"));

        stats
    }

    /// Gather integer values through [ckb-metrics](../../ckb_metrics/index.html)
    fn gather_int_values(&self, _: &str) -> Vec<CfMemStat>;
}

impl<RocksDB> TrackRocksDBMemory for RocksDB
where
    RocksDB: GetColumnFamilys + GetProperty + GetPropertyCF,
{
    fn gather_int_values(&self, key: &str) -> Vec<CfMemStat> {
        let mut stats = Vec::new();
        for (cf_name, cf) in self.get_cfs() {
            let value_col: PropertyValue<u64> = self
                .property_int_value_cf(cf, &format!("rocksdb.{}", key))
                .map_err(|err| format!("{}", err))
                .into();

            stats.push(CfMemStat {
                name: cf_name.to_owned(),
                type_: key.to_owned(),
                value: value_col,
            });
        }
        stats
    }
}
