use ckb_types::core::hardfork::HardForkSwitch;
use tokio::sync::Mutex;

lazy_static::lazy_static! {
    pub static ref GLOBAL_VM_VERSION: Mutex<u32> = Mutex::new(0);
    pub static ref GLOBAL_HARDFORK_SWITCH: Mutex<HardForkSwitch> = Mutex::new(
        HardForkSwitch::new_without_any_enabled()
    );
    pub static ref GLOBAL_CURRENT_EPOCH_NUMBER: Mutex<u64> = Mutex::new(0);
}
