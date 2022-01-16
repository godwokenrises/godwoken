use std::sync::atomic::{AtomicU32, AtomicU64};

use arc_swap::ArcSwap;
use ckb_types::core::hardfork::HardForkSwitch;

lazy_static::lazy_static! {
    pub static ref GLOBAL_VM_VERSION: AtomicU32 = AtomicU32::new(0);
    pub static ref GLOBAL_HARDFORK_SWITCH: ArcSwap<HardForkSwitch> = ArcSwap::from_pointee(
        HardForkSwitch::new_without_any_enabled()
    );
    pub static ref GLOBAL_CURRENT_EPOCH_NUMBER: AtomicU64 = AtomicU64::new(0);
}
