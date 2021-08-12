use opentelemetry::{global, KeyValue};

lazy_static::lazy_static! {
    static ref HANDLER_ALL: [KeyValue; 1] = [KeyValue::new("godwoken", "all")];
}

pub(crate) const METER: &str = "metrics";

pub(crate) fn f64_counter<T: Into<String>>(name: T, val: f64) {
    global_meter()
        .f64_counter(name)
        .init()
        .bind(HANDLER_ALL.as_ref())
        .add(val);
}

pub(crate) fn u64_counter<T: Into<String>>(name: T, val: u64) {
    global_meter()
        .u64_counter(name)
        .init()
        .bind(HANDLER_ALL.as_ref())
        .add(val);
}

pub(crate) fn u64_gauge<T: Into<String>>(name: T, val: u64) {
    global_meter()
        .u64_value_recorder(name)
        .init()
        .bind(HANDLER_ALL.as_ref())
        .record(val);
}

pub(crate) fn f64_gauge<T: Into<String>>(name: T, val: f64) {
    global_meter()
        .f64_value_recorder(name)
        .init()
        .bind(HANDLER_ALL.as_ref())
        .record(val);
}

pub(crate) fn i64_gauge<T: Into<String>>(name: T, val: i64) {
    global_meter()
        .i64_value_recorder(name)
        .init()
        .bind(HANDLER_ALL.as_ref())
        .record(val);
}

pub(crate) fn u64_value_recorder<T: Into<String>>(name: T, val: u64) {
    global_meter()
        .u64_value_recorder(name)
        .init()
        .bind(HANDLER_ALL.as_ref())
        .record(val);
}

pub(crate) fn f64_value_recorder<T: Into<String>>(name: T, val: f64) {
    global_meter()
        .f64_value_recorder(name)
        .init()
        .bind(HANDLER_ALL.as_ref())
        .record(val);
}

pub(crate) fn global_meter() -> opentelemetry::metrics::Meter {
    global::meter(METER)
}
