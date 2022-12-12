use std::fmt;

use autorocks_sys::rocksdb::{Status, Status_Code, Status_SubCode};

pub struct RocksDBStatusError {
    pub(crate) msg: String,
    pub code: Status_Code,
    pub sub_code: Status_SubCode,
}

impl fmt::Debug for RocksDBStatusError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("RocksDBStatusError")
            .field("msg", &self.msg)
            .field("code", &(self.code.clone() as u8))
            .finish()
    }
}

impl fmt::Display for RocksDBStatusError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.msg)
    }
}

impl std::error::Error for RocksDBStatusError {}

pub type Result<T, E = RocksDBStatusError> = std::result::Result<T, E>;

pub fn into_result(status: &Status) -> Result<()> {
    if status.ok() {
        Ok(())
    } else {
        Err(RocksDBStatusError {
            code: status.code(),
            sub_code: status.subcode(),
            msg: status.ToString().to_string_lossy().into(),
        })
    }
}
