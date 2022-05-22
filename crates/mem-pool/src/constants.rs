/// MAX deposits in the mem block
pub const MAX_MEM_BLOCK_DEPOSITS: usize = 50;
/// MAX withdrawals in the mem block
pub const MAX_MEM_BLOCK_WITHDRAWALS: usize = 50;
/// MAX withdrawals in the mem block
pub const MAX_MEM_BLOCK_TXS: usize = 1000;
/// MIN CKB deposit capacity, calculated from custodian cell size
pub const MIN_CKB_DEPOSIT_CAPACITY: u64 = 298_00000000;
/// MIN Simple UDT deposit capacity, calculated from custodian cell size + simple UDT script
pub const MIN_SUDT_DEPOSIT_CAPACITY: u64 = 379_00000000;
/// MAX custodian cells
pub const MAX_CUSTODIANS: usize = 50;
