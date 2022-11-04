use crate::error::Error;
use ckb_std::{
    ckb_constants::Source,
    high_level::{
        load_cell_capacity, load_cell_data, load_cell_lock_hash, load_cell_type_hash, QueryIter,
    },
};

#[derive(PartialEq, Eq, Debug, Clone)]
pub enum TokenType {
    CKB,
    SUDT([u8; 32]),
}

impl From<[u8; 32]> for TokenType {
    fn from(sudt_script_hash: [u8; 32]) -> Self {
        if sudt_script_hash == [0u8; 32] {
            TokenType::CKB
        } else {
            TokenType::SUDT(sudt_script_hash)
        }
    }
}

#[derive(PartialEq, Eq, Debug, Clone)]
pub struct CellTokenAmount {
    pub total_token_amount: u128,
    pub total_capacity: u128,
}

pub fn fetch_token_amount_by_lock_hash(
    owner_lock_hash: &[u8; 32],
    token_type: &TokenType,
    source: Source,
) -> Result<CellTokenAmount, Error> {
    let mut total_token_amount = 0u128;
    let mut total_capacity = 0u128;
    for (i, lock_hash) in QueryIter::new(load_cell_lock_hash, source)
        .into_iter()
        .enumerate()
    {
        if &lock_hash != owner_lock_hash {
            continue;
        }

        let capacity = load_cell_capacity(i, source)?;
        total_capacity = total_capacity
            .checked_add(capacity as u128)
            .ok_or(Error::AmountOverflow)?;
        let amount = match load_cell_type_hash(i, source)? {
            Some(type_hash) if &TokenType::SUDT(type_hash) == token_type => {
                let data = load_cell_data(i, source)?;
                let mut buf = [0u8; 16];
                buf.copy_from_slice(&data[..16]);
                u128::from_le_bytes(buf)
            }
            _ => 0,
        };
        total_token_amount = total_token_amount
            .checked_add(amount)
            .ok_or(Error::AmountOverflow)?;
    }
    Ok(CellTokenAmount {
        total_token_amount,
        total_capacity,
    })
}
