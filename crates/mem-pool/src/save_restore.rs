use crate::mem_block::MemBlock;

use anyhow::Result;
use gw_types::packed;
use gw_types::prelude::Entity;

use std::ffi::OsStr;
use std::fs::{create_dir_all, read, read_dir, remove_file, write};
use std::path::{Path, PathBuf};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

const MEM_BLOCK_FILENAME_PREFIX: &str = "mem_block_timestamp_";
const ONE_HOUR: Duration = Duration::from_secs(60 * 60);

pub struct SaveRestore {
    save_path: PathBuf,
}

impl SaveRestore {
    pub fn build<P: AsRef<Path>>(save_path: &P) -> Result<Self> {
        create_dir_all(save_path.as_ref())?;

        Ok(SaveRestore {
            save_path: save_path.as_ref().to_owned(),
        })
    }

    pub fn save(&self, mem_block: &MemBlock) -> Result<()> {
        let now = SystemTime::now().duration_since(UNIX_EPOCH)?.as_millis();
        self.save_with_timestamp(mem_block, now)
    }

    pub fn save_with_timestamp(&self, mem_block: &MemBlock, timestamp: u128) -> Result<()> {
        let file_path = self.block_file_path(timestamp);

        let packed = mem_block.pack();
        write(file_path, packed.as_slice())?;

        Ok(())
    }

    pub fn restore_from_latest(&self) -> Result<Option<packed::MemBlock>> {
        let mut dir = read_dir(self.save_path.clone())?;
        let mut opt_latest_timestamp = None;
        while let Some(Ok(file)) = dir.next() {
            let file_path = file.path();
            let file_name = match file_path.file_name().map(OsStr::to_str) {
                Some(Some(file_name)) => file_name,
                _ => continue,
            };

            let (_, str_timestamp) = file_name.split_at(MEM_BLOCK_FILENAME_PREFIX.len());
            if let Ok(timestamp) = str_timestamp.parse() {
                if opt_latest_timestamp.is_none() || Some(timestamp) > opt_latest_timestamp {
                    opt_latest_timestamp = Some(timestamp);
                }
            }
        }

        let file_path = match opt_latest_timestamp {
            Some(timestamp) => self.block_file_path(timestamp),
            None => return Ok(None),
        };

        let block = packed::MemBlock::from_slice(&read(file_path)?)?;
        Ok(Some(block))
    }

    pub fn restore_from_timestamp(&self, timestamp: u128) -> Result<Option<packed::MemBlock>> {
        let mut dir = read_dir(self.save_path.clone())?;
        let mut opt_timestamp_found = None;
        while let Some(Ok(file)) = dir.next() {
            let file_path = file.path();
            let file_name = match file_path.file_name().map(OsStr::to_str) {
                Some(Some(file_name)) => file_name,
                _ => continue,
            };

            let (_, str_timestamp) = file_name.split_at(MEM_BLOCK_FILENAME_PREFIX.len());
            if let Ok(file_timestamp) = str_timestamp.parse() {
                if file_timestamp == timestamp {
                    opt_timestamp_found = Some(file_timestamp);
                    break;
                }
            }
        }

        let file_path = match opt_timestamp_found {
            Some(timestamp) => self.block_file_path(timestamp),
            None => return Ok(None),
        };

        let block = packed::MemBlock::from_slice(&read(file_path)?)?;
        Ok(Some(block))
    }

    pub fn delete_before_one_hour(&self) {
        let now = match SystemTime::now().duration_since(UNIX_EPOCH) {
            Ok(duration) => duration,
            Err(err) => {
                log::info!("[mem-pool] save restore error {}", err);
                return;
            }
        };

        let before_one_hour = now.saturating_sub(ONE_HOUR).as_millis();
        self.delete_before_timestamp(before_one_hour);
    }

    pub fn delete_before_timestamp(&self, before_timestamp: u128) {
        let mut dir = match read_dir(self.save_path.clone()) {
            Ok(dir) => dir,
            Err(err) => {
                log::warn!(
                    "[mem-pool] save restore open {:?} error {}",
                    self.save_path,
                    err
                );
                return;
            }
        };

        while let Some(Ok(file)) = dir.next() {
            let file_path = file.path();
            let file_name = match file_path.file_name().map(OsStr::to_str) {
                Some(Some(file_name)) => file_name,
                _ => continue,
            };

            let (_, str_timestamp) = file_name.split_at(MEM_BLOCK_FILENAME_PREFIX.len());
            let timestamp = match str_timestamp.parse() {
                Ok(timestamp) => timestamp,
                Err(_) => continue,
            };

            if timestamp < before_timestamp {
                let file_path = self.block_file_path(timestamp);
                if let Err(err) = remove_file(file_path.clone()) {
                    log::warn!(
                        "[mem-pool] save restore delete {:?} error {}",
                        file_path,
                        err
                    );
                }
            }
        }
    }

    fn block_file_path(&self, timestamp: u128) -> PathBuf {
        let file_name = format!("{}{}", MEM_BLOCK_FILENAME_PREFIX, timestamp);
        let mut file_path = self.save_path.to_owned();
        file_path.push(file_name);
        file_path
    }
}

#[cfg(test)]
mod tests {
    use std::time::{Duration, SystemTime, UNIX_EPOCH};

    use gw_types::prelude::Entity;

    use crate::mem_block::MemBlock;

    use super::SaveRestore;

    #[test]
    fn test_save_restore() {
        let tmp_dir = tempfile::TempDir::new().unwrap();
        let save_restore = SaveRestore::build(&tmp_dir).unwrap();

        // Should able to save and restore packed mem block
        let mem_block = MemBlock::with_block_producer(666);
        let expected_packed = mem_block.pack();
        save_restore.save(&mem_block).unwrap();
        let restored_packed = save_restore.restore_from_latest().unwrap().expect("saved");
        assert_eq!(expected_packed.as_slice(), restored_packed.as_slice());

        // Should restore latest mem block
        let earlier_mem_block = MemBlock::with_block_producer(999);
        let earlier_timestamp = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .saturating_sub(Duration::from_secs(233))
            .as_millis();
        save_restore
            .save_with_timestamp(&earlier_mem_block, earlier_timestamp)
            .unwrap();
        let restored_packed = save_restore.restore_from_latest().unwrap().expect("saved");
        assert_eq!(expected_packed.as_slice(), restored_packed.as_slice());

        // Should able to delete earlier mem block
        let earlier_packed = save_restore
            .restore_from_timestamp(earlier_timestamp)
            .unwrap()
            .expect("earlier timestamp");
        assert_eq!(
            earlier_mem_block.pack().as_slice(),
            earlier_packed.as_slice()
        );
        save_restore.delete_before_timestamp(earlier_timestamp.saturating_add(1000));
        let opt_restored = save_restore
            .restore_from_timestamp(earlier_timestamp)
            .unwrap();
        assert!(opt_restored.is_none());
    }
}
