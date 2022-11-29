/// Fork features collection
pub struct Fork;

impl Fork {
    // Fork feature: block.timestamp < input.since
    pub const fn enforce_block_timestamp_lower_than_since(global_state_version: u8) -> bool {
        global_state_version >= 1
    }

    // Fork feature: block.timestamp in the backbone range
    pub const fn enforce_block_timestamp_in_l1_backbone_range(global_state_version: u8) -> bool {
        global_state_version >= 2
    }

    // Fork feature: use timestamp as timepoint for finality check
    pub const fn use_timestamp_as_timepoint(global_state_version: u8) -> bool {
        global_state_version >= 2
    }
}
