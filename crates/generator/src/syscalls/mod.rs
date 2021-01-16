use crate::traits::CodeStore;
use crate::types::RunResult;
use ckb_vm::{
    memory::Memory,
    registers::{A0, A1, A2, A3, A7},
    Error as VMError, Register, SupportMachine, Syscalls,
};
use gw_common::{
    blake2b::new_blake2b,
    h256_ext::H256Ext,
    state::{
        build_account_field_key, build_script_hash_to_account_id_key, State, GW_ACCOUNT_NONCE,
        GW_ACCOUNT_SCRIPT_HASH,
    },
    H256,
};
use gw_types::{
    bytes::Bytes,
    packed::{BlockInfo, LogItem, RawL2Transaction, Script},
    prelude::*,
};
use std::cmp;

/* Constants */
// 24KB is max ethereum contract code size
const MAX_SET_RETURN_DATA_SIZE: u64 = 1024 * 24;

/* Syscall numbers */
const SYS_STORE: u64 = 3051;
const SYS_LOAD: u64 = 3052;
const SYS_SET_RETURN_DATA: u64 = 3061;
const SYS_CREATE: u64 = 3071;
/* internal syscall numbers */
const SYS_LOAD_TRANSACTION: u64 = 4051;
const SYS_LOAD_BLOCKINFO: u64 = 4052;
const SYS_LOAD_SCRIPT_HASH_BY_ACCOUNT_ID: u64 = 4053;
const SYS_LOAD_ACCOUNT_ID_BY_SCRIPT_HASH: u64 = 4054;
const SYS_LOAD_ACCOUNT_SCRIPT: u64 = 4055;
const SYS_STORE_DATA: u64 = 4056;
const SYS_LOAD_DATA: u64 = 4057;
const SYS_LOG: u64 = 4061;
/* CKB compatible syscalls */
const DEBUG_PRINT_SYSCALL_NUMBER: u64 = 2177;

/* Syscall errors */
pub(crate) const SUCCESS: u8 = 0;
pub(crate) const ERROR_DUPLICATED_SCRIPT_HASH: u8 = std::i8::MAX as u8;

pub(crate) struct L2Syscalls<'a, S> {
    pub(crate) state: &'a S,
    pub(crate) block_info: &'a BlockInfo,
    pub(crate) raw_tx: &'a RawL2Transaction,
    pub(crate) code_store: &'a dyn CodeStore,
    pub(crate) result: &'a mut RunResult,
}

fn load_data_u32<Mac: SupportMachine>(machine: &mut Mac, addr: u64) -> Result<u32, VMError> {
    let mut data = [0u8; 4];
    for (i, c) in data.iter_mut().enumerate() {
        *c = machine
            .memory_mut()
            .load8(&Mac::REG::from_u64(addr).overflowing_add(&Mac::REG::from_u64(i as u64)))?
            .to_u8();
    }
    Ok(u32::from_le_bytes(data))
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

impl<'a, S: State, Mac: SupportMachine> Syscalls<Mac> for L2Syscalls<'a, S> {
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
                let value = self.get_raw(&key)?;
                machine
                    .memory_mut()
                    .store_bytes(value_addr, &value.as_slice())?;
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
            SYS_CREATE => {
                let script_addr = machine.registers()[A0].to_u64();
                let script_len = machine.registers()[A1].to_u32();
                let account_id_addr = machine.registers()[A2].clone();

                let script_data = load_bytes(machine, script_addr, script_len as usize)?;
                let script = Script::from_slice(&script_data[..]).map_err(|err| {
                    eprintln!("syscall error: invalid script to create : {:?}", err);
                    VMError::Unexpected
                })?;
                let script_hash = script.hash();

                // Return error if script_hash is exists
                if self
                    .get_account_id_by_script_hash(&script_hash.into())?
                    .is_some()
                {
                    machine.set_register(A0, Mac::REG::from_u8(ERROR_DUPLICATED_SCRIPT_HASH));
                    return Ok(true);
                }

                // Same logic from State::create_account()
                let id = self.get_account_count()?;
                self.result.write_values.insert(
                    build_account_field_key(id, GW_ACCOUNT_NONCE).into(),
                    H256::zero(),
                );
                self.result.write_values.insert(
                    build_account_field_key(id, GW_ACCOUNT_SCRIPT_HASH).into(),
                    script_hash.into(),
                );
                // script hash to id
                self.result.write_values.insert(
                    build_script_hash_to_account_id_key(&script_hash[..]).into(),
                    H256::from_u32(id),
                );
                self.result
                    .new_scripts
                    .insert(script_hash.into(), script.as_slice().to_vec());
                self.set_account_count(id + 1)?;
                machine
                    .memory_mut()
                    .store32(&account_id_addr, &Mac::REG::from_u32(id))?;
                machine.set_register(A0, Mac::REG::from_u8(SUCCESS));
                Ok(true)
            }
            SYS_LOAD_BLOCKINFO => {
                let data = self.block_info.as_slice();
                store_data(machine, data)?;
                machine.set_register(A0, Mac::REG::from_u8(SUCCESS));
                Ok(true)
            }
            SYS_LOAD_TRANSACTION => {
                let data = self.raw_tx.as_slice();
                store_data(machine, data)?;
                machine.set_register(A0, Mac::REG::from_u8(SUCCESS));
                Ok(true)
            }
            SYS_LOAD_ACCOUNT_ID_BY_SCRIPT_HASH => {
                let script_hash_addr = machine.registers()[A0].to_u64();
                let account_id_addr = machine.registers()[A1].to_u64();
                let script_hash = load_data_h256(machine, script_hash_addr)?;
                let account_id = self
                    .get_account_id_by_script_hash(&script_hash)
                    .map_err(|_err| VMError::Unexpected)?
                    .ok_or_else(|| {
                        eprintln!("returned zero account id");
                        VMError::Unexpected
                    })?;
                machine
                    .memory_mut()
                    .store_bytes(account_id_addr, &account_id.to_le_bytes()[..])?;
                machine.set_register(A0, Mac::REG::from_u8(SUCCESS));
                Ok(true)
            }
            SYS_LOAD_SCRIPT_HASH_BY_ACCOUNT_ID => {
                let account_id = machine.registers()[A0].to_u32();
                let script_hash_addr = machine.registers()[A1].to_u64();
                let script_hash = self.get_script_hash(account_id).map_err(|err| {
                    eprintln!("syscall error: get script hash by account id: {:?}", err);
                    VMError::Unexpected
                })?;
                machine
                    .memory_mut()
                    .store_bytes(script_hash_addr, script_hash.as_slice())?;
                machine.set_register(A0, Mac::REG::from_u8(SUCCESS));
                Ok(true)
            }
            SYS_LOAD_ACCOUNT_SCRIPT => {
                let account_id = machine.registers()[A0].to_u32();
                let len_addr = machine.registers()[A1].to_u64();
                let offset = machine.registers()[A2].to_u32() as usize;
                let script_addr = machine.registers()[A3].to_u64();

                let script_hash = self.get_script_hash(account_id).map_err(|err| {
                    eprintln!("syscall error: get script hash by account id: {:?}", err);
                    VMError::Unexpected
                })?;
                let len = load_data_u32(machine, len_addr)? as usize;
                let script = self.get_script(&script_hash).ok_or_else(|| {
                    eprintln!(
                        "syscall error: script not found by script hash: {:?}",
                        script_hash
                    );
                    VMError::Unexpected
                })?;
                let data = script.as_slice();
                let new_len = if offset >= data.len() {
                    0
                } else if (offset + len) > data.len() {
                    data.len() - offset
                } else {
                    len
                };
                if new_len > 0 {
                    machine
                        .memory_mut()
                        .store_bytes(script_addr, &data[offset..offset + new_len])?;
                }
                machine
                    .memory_mut()
                    .store_bytes(len_addr, &(new_len as u32).to_le_bytes())?;
                machine.set_register(A0, Mac::REG::from_u8(SUCCESS));
                Ok(true)
            }
            SYS_STORE_DATA => {
                let data_len = machine.registers()[A0].to_u32();
                let data_addr = machine.registers()[A1].to_u64();

                let data = load_bytes(machine, data_addr, data_len as usize)?;
                let mut data_hash = [0u8; 32];
                let mut hasher = new_blake2b();
                hasher.update(data.as_ref());
                hasher.finalize(&mut data_hash);
                self.result
                    .write_data
                    .insert(data_hash.into(), data.as_slice().to_vec());
                machine.set_register(A0, Mac::REG::from_u8(SUCCESS));
                Ok(true)
            }
            SYS_LOAD_DATA => {
                let data_hash_addr = machine.registers()[A0].to_u64();
                let len_addr = machine.registers()[A1].to_u64();
                let offset = machine.registers()[A2].to_u32() as usize;
                let data_addr = machine.registers()[A3].to_u64();

                let data_hash = load_data_h256(machine, data_hash_addr)?;
                let len = load_data_u32(machine, len_addr)? as usize;
                let data = self.get_data(&data_hash).ok_or_else(|| {
                    eprintln!(
                        "syscall error: data not found by data hash: {:?}",
                        data_hash
                    );
                    VMError::Unexpected
                })?;
                let data_ref = data.as_ref();
                let new_len = if offset >= data_ref.len() {
                    0
                } else if (offset + len) > data_ref.len() {
                    data_ref.len() - offset
                } else {
                    len
                };
                if new_len > 0 {
                    machine
                        .memory_mut()
                        .store_bytes(data_addr, &data_ref[offset..offset + new_len])?;
                }
                machine
                    .memory_mut()
                    .store_bytes(len_addr, &(new_len as u32).to_le_bytes())?;
                self.result.read_data.insert(data_hash, data.len());
                machine.set_register(A0, Mac::REG::from_u8(SUCCESS));
                Ok(true)
            }
            SYS_LOG => {
                let account_id = machine.registers()[A0].to_u32();
                let data_len = machine.registers()[A1].to_u32();
                let data_addr = machine.registers()[A2].to_u64();

                let data = load_bytes(machine, data_addr, data_len as usize)?;
                self.result.logs.push(
                    LogItem::new_builder()
                        .account_id(account_id.pack())
                        .data(Bytes::from(data).pack())
                        .build(),
                );
                machine.set_register(A0, Mac::REG::from_u8(SUCCESS));
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

impl<'a, S: State> L2Syscalls<'a, S> {
    fn get_raw(&mut self, key: &H256) -> Result<H256, VMError> {
        let value = match self.result.write_values.get(&key) {
            Some(value) => *value,
            None => {
                let tree_value = self.state.get_raw(&key).map_err(|_| VMError::Unexpected)?;
                self.result.read_values.insert(*key, tree_value);
                tree_value
            }
        };
        Ok(value)
    }
    fn get_account_count(&self) -> Result<u32, VMError> {
        if let Some(id) = self.result.account_count {
            Ok(id)
        } else {
            self.state.get_account_count().map_err(|err| {
                eprintln!("syscall error: get account count : {:?}", err);
                VMError::Unexpected
            })
        }
    }
    fn set_account_count(&mut self, count: u32) -> Result<(), VMError> {
        self.result.account_count = Some(count);
        Ok(())
    }
    fn get_script(&self, script_hash: &H256) -> Option<Script> {
        self.result
            .new_scripts
            .get(script_hash)
            .map(|data| Script::from_slice(&data).expect("Script"))
            .or_else(|| self.code_store.get_script(&script_hash))
    }
    fn get_data(&self, data_hash: &H256) -> Option<Bytes> {
        self.result
            .write_data
            .get(data_hash)
            .map(|data| Bytes::from(data.clone()))
            .or_else(|| self.code_store.get_data(&data_hash))
    }
    fn get_script_hash(&mut self, id: u32) -> Result<H256, VMError> {
        let value = self
            .get_raw(&build_account_field_key(id, GW_ACCOUNT_SCRIPT_HASH).into())
            .map_err(|err| {
                eprintln!("syscall error: get script hash by account id : {:?}", err);
                VMError::Unexpected
            })?;
        Ok(value.into())
    }
    fn get_account_id_by_script_hash(
        &mut self,
        script_hash: &H256,
    ) -> Result<Option<u32>, VMError> {
        let value = self
            .get_raw(&build_script_hash_to_account_id_key(script_hash.as_slice()).into())
            .map_err(|err| {
                eprintln!("syscall error: get account id by script hash : {:?}", err);
                VMError::Unexpected
            })?;
        if value.is_zero() {
            return Ok(None);
        }
        let id = value.to_u32();
        Ok(Some(id))
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
