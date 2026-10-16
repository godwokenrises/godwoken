#![no_std]
#![no_main]

use ckb_std::ckb_types::prelude::Entity as CkbEntity;
use ckb_std::{
    ckb_constants::{CellField, Source},
    error::SysError,
    high_level, syscalls,
};
use exit_withdraw_state_validator::v0 as settlement;
use exit_withdraw_state_validator::v0 as codec;
use exit_withdraw_state_validator::*;
use gw_types::{packed, prelude::*};
ckb_std::entry!(entry);
ckb_std::default_alloc!(4096, 2097152, 64);

fn syscall<T>(r: core::result::Result<T, SysError>) -> Result<T> {
    r.map_err(|_| Error::Syscall)
}
fn bounded_data(index: usize, source: Source, limit: usize) -> Result<alloc::vec::Vec<u8>> {
    let n = match syscalls::load_cell_data(&mut [], 0, index, source) {
        Ok(n) => n,
        Err(SysError::LengthNotEnough(n)) => n,
        Err(_) => return Err(Error::Syscall),
    };
    if n > limit {
        return Err(Error::Limit);
    }
    syscall(high_level::load_cell_data(index, source))
}
fn bounded_script(
    index: usize,
    source: Source,
    field: CellField,
) -> Result<Option<packed::Script>> {
    let n = match syscalls::load_cell_by_field(&mut [], 0, index, source, field) {
        Ok(n) | Err(SysError::LengthNotEnough(n)) => n,
        Err(SysError::ItemMissing) if field == CellField::Type => return Ok(None),
        Err(_) => return Err(Error::Syscall),
    };
    if n > MAX_SCRIPT {
        return Err(Error::Limit);
    }
    let mut bytes = alloc::vec![0;n];
    syscall(syscalls::load_cell_by_field(
        &mut bytes, 0, index, source, field,
    ))?;
    packed::ScriptReader::verify(&bytes, false).map_err(|_| Error::Encoding)?;
    Ok(Some(packed::Script::new_unchecked(bytes.into())))
}
fn cells(source: Source, max: usize) -> Result<alloc::vec::Vec<Cell>> {
    let mut out = alloc::vec::Vec::new();
    for i in 0..=max {
        let cap = match high_level::load_cell_capacity(i, source) {
            Ok(v) => v,
            Err(SysError::IndexOutOfBound) => return Ok(out),
            Err(_) => return Err(Error::Syscall),
        };
        if i == max {
            return Err(Error::Limit);
        }
        let lock = bounded_script(i, source, CellField::Lock)?.ok_or(Error::Cell)?;
        let type_ = bounded_script(i, source, CellField::Type)?;
        out.push(Cell {
            capacity: cap,
            lock,
            type_,
            data: bounded_data(i, source, MAX_CELL_DATA)?,
        });
    }
    Err(Error::Limit)
}
fn dependency(expected: Hash, limit: usize) -> Result<alloc::vec::Vec<u8>> {
    for i in 0..1024 {
        match high_level::load_cell_data_hash(i, Source::CellDep) {
            Ok(h) if h == expected => return bounded_data(i, Source::CellDep, limit),
            Ok(_) => {}
            Err(SysError::IndexOutOfBound) => return Err(Error::Profile),
            Err(_) => return Err(Error::Syscall),
        }
    }
    Err(Error::Limit)
}
fn run() -> Result<()> {
    if PROFILE_HASH == [0; 32] {
        return Err(Error::Profile);
    }
    // Every Begin and Pay references the same profile data through cell_deps.
    // PROFILE_HASH is pinned in this binary, so callers cannot substitute settings.
    let settlement_profile =
        codec::decode_profile(&dependency(PROFILE_HASH, MAX_PROFILE)?, PROFILE_HASH)?;
    let p = &settlement_profile.settlement.base;
    let script_size = match syscalls::load_script(&mut [], 0) {
        Ok(n) | Err(SysError::LengthNotEnough(n)) => n,
        Err(_) => return Err(Error::Syscall),
    };
    if script_size > MAX_SCRIPT {
        return Err(Error::Limit);
    }
    let own = syscall(high_level::load_script())?;
    if own.as_slice() != p.rollup.as_slice() {
        return Err(Error::Rollup);
    }
    let config = dependency(p.config_hash, MAX_PROFILE)?;
    let inputs = cells(Source::Input, MAX_INPUTS)?;
    let outputs = cells(Source::Output, MAX_OUTPUTS)?;
    let mut outpoints = alloc::vec::Vec::new();
    for i in 0..inputs.len() {
        let input = syscall(high_level::load_input(i, Source::Input))?;
        outpoints.push(
            packed::OutPoint::from_slice(input.previous_output().as_slice())
                .map_err(|_| Error::Encoding)?,
        );
    }
    let index = outputs
        .iter()
        .position(|c| {
            c.type_
                .as_ref()
                .is_some_and(|s| s.as_slice() == p.rollup.as_slice())
        })
        .ok_or(Error::Rollup)?;
    let n = match syscalls::load_witness(&mut [], 0, index, Source::Output) {
        Ok(n) => n,
        Err(SysError::LengthNotEnough(n)) => n,
        Err(_) => return Err(Error::Syscall),
    };
    if n > MAX_ACTION + 4096 {
        return Err(Error::Limit);
    }
    let w = syscall(high_level::load_witness_args(index, Source::Output))?;
    let b = w.output_type().to_opt().ok_or(Error::Encoding)?.raw_data();
    let b = transport::unframe(&b, inputs.len(), |i| {
        let size = match syscalls::load_witness(&mut [], 0, i, Source::Input) {
            Ok(n) | Err(SysError::LengthNotEnough(n)) => n,
            _ => return Err(Error::Syscall),
        };
        if size > transport::CHUNK {
            return Err(Error::Limit);
        }
        syscall(high_level::load_witness(i, Source::Input))
    })?;
    let a = codec::decode_action(&b)?;
    settlement::verify(
        &settlement_profile,
        PROFILE_HASH,
        &config,
        &Transaction {
            inputs,
            outputs,
            outpoints,
        },
        &a,
    )?;
    Ok(())
}
fn entry() -> i8 {
    match run() {
        Ok(()) => 0,
        Err(e) => e as i8,
    }
}
