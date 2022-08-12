use std::sync::atomic::{AtomicU32, AtomicU64};

use arc_swap::ArcSwap;
use ckb_types::core::hardfork::HardForkSwitch;

lazy_static::lazy_static! {
    pub static ref GLOBAL_VM_VERSION: AtomicU32 = AtomicU32::new(0);
    // https://github.com/nervosnetwork/ckb/blob/v0.100.0/util/types/src/core/hardfork.rs#L171-L183
    pub static ref GLOBAL_HARDFORK_SWITCH: ArcSwap<HardForkSwitch> = ArcSwap::from_pointee(
        HardForkSwitch::new_builder()
            .disable_rfc_0028()
            .disable_rfc_0029()
            .disable_rfc_0030()
            .disable_rfc_0031()
            .disable_rfc_0032()
            .disable_rfc_0036()
            .disable_rfc_0038()
            .build()
            .unwrap()
    );
    pub static ref GLOBAL_CURRENT_EPOCH_NUMBER: AtomicU64 = AtomicU64::new(0);
}
