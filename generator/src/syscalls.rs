use crate::bytes::Bytes;
use crate::smt::SMT;
use crate::state::State;
use ckb_vm::{
    memory::{Memory, FLAG_EXECUTABLE, FLAG_FREEZED},
    registers::{A0, A1, A2, A3, A4, A7},
    Error as VMError, Register, SupportMachine, Syscalls,
};
use gw_types::{
    packed::{BlockInfo, CallContext},
    prelude::*,
};
use sparse_merkle_tree::{traits::Store, H256};
use std::cmp;
use std::collections::HashMap;

/* Constants */
const MAX_SET_RETURN_DATA_SIZE: u64 = 1024;

/* Syscall numbers */
const SYS_STORE: u64 = 3051;
const SYS_LOAD: u64 = 3052;
const SYS_SET_RETURN_DATA: u64 = 3061;
/* internal syscall numbers */
const SYS_LOAD_CALLCONTEXT: u64 = 4051;
const SYS_LOAD_BLOCKINFO: u64 = 4052;
const SYS_LOAD_PROGRAM_AS_DATA: u64 = 4061;
const SYS_LOAD_PROGRAM_AS_CODE: u64 = 4062;
/* CKB compatible syscalls */
const DEBUG_PRINT_SYSCALL_NUMBER: u64 = 2177;

/* Syscall errors */
const SUCCESS: u8 = 0;
const INDEX_OUT_OF_BOUND: u8 = 1;
const ITEM_MISSING: u8 = 2;
const SLICE_OUT_OF_BOUND: u8 = 3;

#[derive(Debug, PartialEq, Clone, Eq, Default)]
pub struct RunResult {
    pub read_values: HashMap<H256, H256>,
    pub write_values: HashMap<H256, H256>,
    pub return_data: Vec<u8>,
}

pub(crate) struct L2Syscalls<'a, S> {
    pub(crate) tree: &'a SMT<S>,
    pub(crate) block_info: &'a BlockInfo,
    pub(crate) call_context: &'a CallContext,
    pub(crate) contracts_by_code_hash: &'a HashMap<[u8; 32], Bytes>,
    pub(crate) result: &'a mut RunResult,
}

fn load_data_h256<Mac: SupportMachine>(machine: &mut Mac, addr: u64) -> Result<H256, VMError> {
    let mut data = [0u8; 32];
    for (i, c) in data.iter_mut().enumerate() {
        *c = machine
            .memory_mut()
            .load8(&Mac::REG::from_u64(addr).overflowing_add(&Mac::REG::from_u64(i as u64)))?
            .to_u8();
    }
    Ok(H256::from(data))
}

fn load_bytes<Mac: SupportMachine>(
    machine: &mut Mac,
    addr: u64,
    len: usize,
) -> Result<Vec<u8>, VMError> {
    let mut data = Vec::with_capacity(len);
    data.resize(len, 0);
    for i in 0..len {
        data[i] = machine
            .memory_mut()
            .load8(&Mac::REG::from_u64(addr).overflowing_add(&Mac::REG::from_u64(i as u64)))?
            .to_u8();
    }
    Ok(data)
}

pub fn store_data<Mac: SupportMachine>(machine: &mut Mac, data: &[u8]) -> Result<u64, VMError> {
    let addr = machine.registers()[A0].to_u64();
    let size_addr = machine.registers()[A1].clone();
    let data_len = data.len() as u64;
    let offset = cmp::min(data_len, machine.registers()[A2].to_u64());

    let size = machine.memory_mut().load64(&size_addr)?.to_u64();
    let full_size = data_len - offset;
    let real_size = cmp::min(size, full_size);
    machine
        .memory_mut()
        .store64(&size_addr, &Mac::REG::from_u64(full_size))?;
    machine
        .memory_mut()
        .store_bytes(addr, &data[offset as usize..(offset + real_size) as usize])?;
    Ok(real_size)
}

impl<'a, S: Store<H256>, Mac: SupportMachine> Syscalls<Mac> for L2Syscalls<'a, S> {
    fn initialize(&mut self, _machine: &mut Mac) -> Result<(), VMError> {
        Ok(())
    }

    fn ecall(&mut self, machine: &mut Mac) -> Result<bool, VMError> {
        let code = machine.registers()[A7].to_u64();
        match code {
            SYS_STORE => {
                let key_addr = machine.registers()[A0].to_u64();
                let key = load_data_h256(machine, key_addr)?;
                let value_addr = machine.registers()[A1].to_u64();
                let value = load_data_h256(machine, value_addr)?;
                self.result.write_values.insert(key, value);
                machine.set_register(A0, Mac::REG::from_u8(SUCCESS));
                Ok(true)
            }
            SYS_LOAD => {
                let key_addr = machine.registers()[A0].to_u64();
                let key = load_data_h256(machine, key_addr)?;
                let value_addr = machine.registers()[A1].to_u64();
                let value = match self.result.write_values.get(&key) {
                    Some(value) => *value,
                    None => {
                        let tree_value = self.tree.get(&key).map_err(|_| VMError::Unexpected)?;
                        self.result.read_values.insert(key, tree_value);
                        tree_value
                    }
                };
                machine
                    .memory_mut()
                    .store_bytes(value_addr, value.as_slice())?;
                machine.set_register(A0, Mac::REG::from_u8(SUCCESS));
                Ok(true)
            }
            SYS_SET_RETURN_DATA => {
                let data_addr = machine.registers()[A0].to_u64();
                let len = machine.registers()[A1].to_u64();
                if len > MAX_SET_RETURN_DATA_SIZE {
                    return Err(VMError::Unexpected);
                }
                let data = load_bytes(machine, data_addr, len as usize)?;
                self.result.return_data = data;
                machine.set_register(A0, Mac::REG::from_u8(SUCCESS));
                Ok(true)
            }
            SYS_LOAD_BLOCKINFO => {
                let data = self.block_info.as_slice();
                store_data(machine, data)?;
                machine.set_register(A0, Mac::REG::from_u8(SUCCESS));
                Ok(true)
            }
            SYS_LOAD_CALLCONTEXT => {
                let data = self.call_context.as_slice();
                store_data(machine, data)?;
                machine.set_register(A0, Mac::REG::from_u8(SUCCESS));
                Ok(true)
            }
            SYS_LOAD_PROGRAM_AS_DATA => {
                self.load_program_as_data(machine)?;
                Ok(true)
            }
            SYS_LOAD_PROGRAM_AS_CODE => {
                self.load_program_as_code(machine)?;
                Ok(true)
            }
            DEBUG_PRINT_SYSCALL_NUMBER => {
                self.output_debug(machine)?;
                Ok(true)
            }
            _ => Ok(false),
        }
    }
}

impl<'a, S: Store<H256>> L2Syscalls<'a, S> {
    fn load_program_as_code<Mac: SupportMachine>(&self, machine: &mut Mac) -> Result<(), VMError> {
        let addr = machine.registers()[A0].to_u64();
        let memory_size = machine.registers()[A1].to_u64();
        let content_offset = machine.registers()[A2].to_u64();
        let content_size = machine.registers()[A3].to_u64();
        let id: u32 = machine.registers()[A4].to_u64() as u32;

        let code_hash = self.tree.get_code_hash(id).map_err(|err| {
            eprintln!("syscall error: get code hash : {:?}", err);
            VMError::Unexpected
        })?;
        let program = self.contracts_by_code_hash.get(&code_hash).ok_or_else(|| {
            eprintln!("syscall error: can't find code_hash : {:?}", code_hash);
            VMError::Unexpected
        })?;

        let content_end = content_offset
            .checked_add(content_size)
            .ok_or(VMError::OutOfBound)?;
        if content_offset >= program.len() as u64
            || content_end > program.len() as u64
            || content_size > memory_size
        {
            machine.set_register(A0, Mac::REG::from_u8(SLICE_OUT_OF_BOUND));
            return Ok(());
        }
        let data = program.slice((content_offset as usize)..(content_end as usize));
        machine.memory_mut().init_pages(
            addr,
            memory_size,
            FLAG_EXECUTABLE | FLAG_FREEZED,
            Some(data),
            0,
        )?;

        machine.set_register(A0, Mac::REG::from_u8(SUCCESS));
        Ok(())
    }

    fn load_program_as_data<Mac: SupportMachine>(&self, machine: &mut Mac) -> Result<(), VMError> {
        let id: u32 = machine.registers()[A3].to_u64() as u32;

        let code_hash = self.tree.get_code_hash(id).map_err(|err| {
            eprintln!("syscall error: get code hash : {:?}", err);
            VMError::Unexpected
        })?;
        let program = self.contracts_by_code_hash.get(&code_hash).ok_or_else(|| {
            eprintln!("syscall error: can't find code_hash : {:?}", code_hash);
            VMError::Unexpected
        })?;
        store_data(machine, program)?;
        machine.set_register(A0, Mac::REG::from_u8(SUCCESS));
        Ok(())
    }

    fn output_debug<Mac: SupportMachine>(&self, machine: &mut Mac) -> Result<(), VMError> {
        let mut addr = machine.registers()[A0].to_u64();
        let mut buffer = Vec::new();

        loop {
            let byte = machine
                .memory_mut()
                .load8(&Mac::REG::from_u64(addr))?
                .to_u8();
            if byte == 0 {
                break;
            }
            buffer.push(byte);
            addr += 1;
        }

        let s = String::from_utf8(buffer).map_err(|_| VMError::ParseError)?;
        println!("[contract debug]: {}", s);
        Ok(())
    }
}
