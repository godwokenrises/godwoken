use gw_config::FeeConfig;

#[derive(Default, Clone)]
pub struct FeeConfigManager {
    fee_config: FeeConfig,
}

impl FeeConfigManager {
    pub fn create(fee_config: FeeConfig) -> FeeConfigManager {
        Self { fee_config }
    }

    pub fn get_fee_config(&self) -> &FeeConfig {
        &self.fee_config
    }

    // Returns old config.
    pub fn reload(&mut self, fee_config: FeeConfig) -> FeeConfig {
        let old_config = self.fee_config.clone();
        self.fee_config = fee_config;
        old_config
    }
}
