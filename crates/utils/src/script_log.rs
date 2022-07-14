use anyhow::{anyhow, Result};
use gw_common::registry_address::RegistryAddress;
use gw_common::H256;
use gw_types::packed::LogItem;
use gw_types::prelude::*;
use gw_types::U256;
use std::convert::TryInto;
use std::usize;

pub const GW_LOG_SUDT_TRANSFER: u8 = 0x0;
pub const GW_LOG_SUDT_PAY_FEE: u8 = 0x1;
pub const GW_LOG_POLYJUICE_SYSTEM: u8 = 0x2;
pub const GW_LOG_POLYJUICE_USER: u8 = 0x3;

#[derive(Debug, Clone)]
pub enum GwLog {
    SudtTransfer {
        sudt_id: u32,
        from_address: RegistryAddress,
        to_address: RegistryAddress,
        amount: U256,
    },
    SudtPayFee {
        sudt_id: u32,
        from_address: RegistryAddress,
        block_producer_address: RegistryAddress,
        amount: U256,
    },
    PolyjuiceSystem {
        gas_used: u64,
        cumulative_gas_used: u64,
        created_address: [u8; 20],
        status_code: u32,
    },
    PolyjuiceUser {
        address: [u8; 20],
        data: Vec<u8>,
        topics: Vec<H256>,
    },
}

fn parse_sudt_log_data(data: &[u8]) -> Result<(RegistryAddress, RegistryAddress, U256)> {
    let data_len_err_msg = "sudt log data too short";

    let mut start = 0;
    let mut end = start + {
        let from_address_byte_size = u32::from_le_bytes(
            data.get(4..8)
                .ok_or_else(|| anyhow!(data_len_err_msg))?
                .try_into()?,
        );
        if from_address_byte_size == 0 {
            8
        } else {
            28
        }
    };

    let from_address = match RegistryAddress::from_slice(
        data.get(start..end)
            .ok_or_else(|| anyhow!(data_len_err_msg))?,
    ) {
        Some(registry_address) => registry_address,
        None => {
            return Err(anyhow!("parse from address error"));
        }
    };

    start = end;
    end = start + {
        let to_address_byte_size = u32::from_le_bytes(
            data.get(start + 4..start + 8)
                .ok_or_else(|| anyhow!(data_len_err_msg))?
                .try_into()?,
        );
        if to_address_byte_size == 0 {
            8
        } else {
            28
        }
    };

    let to_address = match RegistryAddress::from_slice(
        data.get(start..end)
            .ok_or_else(|| anyhow!(data_len_err_msg))?,
    ) {
        Some(registry_address) => registry_address,
        None => {
            return Err(anyhow!("parse to address error"));
        }
    };

    let mut u256_bytes = [0u8; 32];
    u256_bytes.copy_from_slice(
        data.get(end..(end + 32))
            .ok_or_else(|| anyhow!(data_len_err_msg))?,
    );
    let amount = U256::from_little_endian(&u256_bytes);
    Ok((from_address, to_address, amount))
}

pub fn parse_log(item: &LogItem) -> Result<GwLog> {
    let service_flag: u8 = item.service_flag().into();
    let raw_data = item.data().raw_data();
    let data = raw_data.as_ref();
    match service_flag {
        GW_LOG_SUDT_TRANSFER => {
            let sudt_id: u32 = item.account_id().unpack();
            let data_len = data.len();
            // 28 + 28 + 32 = 88
            // 8 + 28 + 32 = 68
            // 28 + 8 + 32 = 68
            // 8 + 8 + 32 = 48
            if data_len != 88 && data_len != 68 && data_len != 48 {
                return Err(anyhow!(
                    "Invalid data length: {}, data: {}",
                    data.len(),
                    hex(data)?
                ));
            }
            let (from_address, to_address, amount) = parse_sudt_log_data(data)?;
            Ok(GwLog::SudtTransfer {
                sudt_id,
                from_address,
                to_address,
                amount,
            })
        }
        GW_LOG_SUDT_PAY_FEE => {
            let sudt_id: u32 = item.account_id().unpack();
            let data_len = data.len();
            // 28 + 28 + 32 = 88
            // 8 + 28 + 32 = 68
            // 28 + 8 + 32 = 68
            // 8 + 8 + 32 = 48
            if data_len != 88 && data_len != 68 && data_len != 48 {
                return Err(anyhow!(
                    "Invalid data length: {}, data: {}",
                    data.len(),
                    hex(data)?
                ));
            }
            let (from_address, block_producer_address, amount) = parse_sudt_log_data(data)?;
            Ok(GwLog::SudtPayFee {
                sudt_id,
                from_address,
                block_producer_address,
                amount,
            })
        }
        GW_LOG_POLYJUICE_SYSTEM => {
            if data.len() != (8 + 8 + 20 + 4) {
                return Err(anyhow!(
                    "invalid system log raw data length: {}, data: {}",
                    data.len(),
                    hex(data)?,
                ));
            }

            let mut u64_bytes = [0u8; 8];
            u64_bytes.copy_from_slice(&data[0..8]);
            let gas_used = u64::from_le_bytes(u64_bytes);
            u64_bytes.copy_from_slice(&data[8..16]);
            let cumulative_gas_used = u64::from_le_bytes(u64_bytes);

            let created_address = {
                let mut buf = [0u8; 20];
                buf.copy_from_slice(&data[16..36]);
                buf
            };
            let mut u32_bytes = [0u8; 4];
            u32_bytes.copy_from_slice(&data[36..40]);
            let status_code = u32::from_le_bytes(u32_bytes);
            Ok(GwLog::PolyjuiceSystem {
                gas_used,
                cumulative_gas_used,
                created_address,
                status_code,
            })
        }
        GW_LOG_POLYJUICE_USER => {
            if data.len() < 24 {
                return Err(anyhow!("invalid user log data length: {}", data.len()));
            }
            let mut offset: usize = 0;
            let mut address = [0u8; 20];
            address.copy_from_slice(&data[offset..offset + 20]);
            offset += 20;
            let mut data_size_bytes = [0u8; 4];
            data_size_bytes.copy_from_slice(&data[offset..offset + 4]);
            offset += 4;
            let data_size: u32 = u32::from_le_bytes(data_size_bytes);
            if data.len() < offset + data_size as usize {
                return Err(anyhow!("invalid user log data size: {}", data_size));
            }
            let mut log_data = vec![0u8; data_size as usize];
            log_data.copy_from_slice(&data[offset..offset + (data_size as usize)]);
            offset += data_size as usize;
            log::debug!("data_size: {}", data_size);

            let mut topics_count_bytes = [0u8; 4];
            topics_count_bytes.copy_from_slice(&data[offset..offset + 4]);
            offset += 4;
            let topics_count: u32 = u32::from_le_bytes(topics_count_bytes);
            let mut topics = Vec::new();
            log::debug!("topics_count: {}", topics_count);
            for _ in 0..topics_count {
                if data.len() < offset + 32 {
                    return Err(anyhow!("invalid user log data"));
                }
                let mut topic = [0u8; 32];
                topic.copy_from_slice(&data[offset..offset + 32]);
                offset += 32;
                topics.push(topic.into());
            }
            if offset != data.len() {
                return Err(anyhow!(
                    "Too many bytes for polyjuice user log data: offset={}, data.len()={}",
                    offset,
                    data.len()
                ));
            }
            Ok(GwLog::PolyjuiceUser {
                address,
                data: log_data,
                topics,
            })
        }
        _ => Err(anyhow!("invalid log service flag: {}", service_flag)),
    }
}

pub fn generate_polyjuice_system_log(
    account_id: u32,
    gas_used: u64,
    cumulative_gas_used: u64,
    created_address: [u8; 20],
    status_code: u32,
) -> LogItem {
    let service_flag: u8 = GW_LOG_POLYJUICE_SYSTEM;
    let mut data = [0u8; 40];
    data[0..8].copy_from_slice(&gas_used.to_le_bytes());
    data[8..16].copy_from_slice(&cumulative_gas_used.to_le_bytes());
    data[16..36].copy_from_slice(&created_address);
    data[36..40].copy_from_slice(&status_code.to_le_bytes());
    LogItem::new_builder()
        .account_id(account_id.pack())
        .service_flag(service_flag.into())
        .data(data.pack())
        .build()
}

fn hex(raw: &[u8]) -> Result<String> {
    Ok(format!("0x{}", faster_hex::hex_string(raw)?))
}
