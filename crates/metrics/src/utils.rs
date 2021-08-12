use std::fmt::Debug;

pub fn f64_counter<T: Into<String> + Debug>(name: T, val: f64) {
    #[cfg(feature = "default")]
    log::debug!("[metrics]: [{:?}], val: {:?}", &name, val);
    #[cfg(feature = "metrics")]
    crate::opentelemeter::utils::f64_counter(name, val);
}

pub fn u64_counter<T: Into<String> + Debug>(name: T, val: u64) {
    #[cfg(feature = "default")]
    log::debug!("[metrics]: [{:?}], val: {:?}", &name, val);
    #[cfg(feature = "metrics")]
    crate::opentelemeter::utils::u64_counter(name, val);
}

pub fn f64_gauge<T: Into<String> + Debug>(name: T, val: f64) {
    #[cfg(feature = "default")]
    log::debug!("[metrics]: [{:?}], val: {:?}", &name, val);
    #[cfg(feature = "metrics")]
    crate::opentelemeter::utils::f64_gauge(name, val);
}

pub fn u64_gauge<T: Into<String> + Debug>(name: T, val: u64) {
    #[cfg(feature = "default")]
    log::debug!("[metrics]: [{:?}], val: {:?}", &name, val);
    #[cfg(feature = "metrics")]
    crate::opentelemeter::utils::u64_gauge(name, val);
}

pub fn i64_gauge<T: Into<String> + Debug>(name: T, val: i64) {
    #[cfg(feature = "default")]
    log::debug!("[metrics]: [{:?}], val: {:?}", &name, val);
    #[cfg(feature = "metrics")]
    crate::opentelemeter::utils::i64_gauge(name, val);
}

pub fn u64_value_recorder<T: Into<String> + Debug>(name: T, val: u64) {
    #[cfg(feature = "default")]
    log::debug!("[metrics]: [{:?}], val: {:?}", &name, val);
    #[cfg(feature = "metrics")]
    crate::opentelemeter::utils::u64_value_recorder(name, val);
}

pub fn f64_value_recorder<T: Into<String> + Debug>(name: T, val: f64) {
    #[cfg(feature = "default")]
    log::debug!("[metrics]: [{:?}], val: {:?}", &name, val);
    #[cfg(feature = "metrics")]
    crate::opentelemeter::utils::f64_value_recorder(name, val);
}
