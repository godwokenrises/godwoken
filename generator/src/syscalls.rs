use crate::blake2b::new_blake2b;
use crate::smt::SMT;
use ckb_vm::{
    registers::{A0, A1, A2, A7},
    Error as VMError, Memory, Register, SupportMachine, Syscalls,
};
use godwoken_types::{
    packed::{BlockInfo, CallContext},
    prelude::*,
};
use sparse_merkle_tree::{traits::Store, H256};
use std::cmp;
use std::collections::{HashMap, HashSet};

/* Constants */
const MAX_SET_RETURN_DATA_SIZE: u64 = 1024;

/* Syscall numbers */
const SYS_STORE: u64 = 3051;
const SYS_LOAD: u64 = 3052;
const SYS_LOAD_CALLCONTEXT: u64 = 3061;
const SYS_LOAD_BLOCKINFO: u64 = 3062;
const SYS_SET_RETURN_DATA: u64 = 3071;

/* Key type */
const GW_ACCOUNT_KV: u8 = 0;
const GW_ACCOUNT_NONCE: u8 = 1;
const GW_ACCOUNT_PUBKEY_HASH: u8 = 2;
const GW_ACCOUNT_CODE_HASH: u8 = 3;

/* Generate raw key
 * raw_key: blake2b(id | type | key)
 *
 * We use raw key in the underlying KV store
 */
fn build_raw_key(id: u32, key: &[u8]) -> [u8; 32] {
    let mut raw_key = [0u8; 32];
    let mut hasher = new_blake2b();
    hasher.update(&id.to_le_bytes());
    hasher.update(&[GW_ACCOUNT_KV]);
    hasher.update(key);
    hasher.finalize(&mut raw_key);
    raw_key
}

#[derive(Debug, PartialEq, Clone, Eq, Default)]
pub struct RunResult {
    pub read_values: HashMap<H256, H256>,
    pub write_values: HashMap<H256, H256>,
    pub return_data: Vec<u8>,
}

pub(crate) struct L2Syscalls<'a, S: Store<H256>> {
    pub(crate) tree: &'a SMT<S>,
    pub(crate) block_info: &'a BlockInfo,
    pub(crate) call_context: &'a CallContext,
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
                let key = {
                    let key = load_data_h256(machine, key_addr)?;
                    build_raw_key(self.call_context.to_id().unpack(), key.as_slice()).into()
                };
                let value_addr = machine.registers()[A1].to_u64();
                let value = load_data_h256(machine, value_addr)?;
                self.result.write_values.insert(key, value);
                machine.set_register(A0, Mac::REG::from_u64(0));
                Ok(true)
            }
            SYS_LOAD => {
                let key_addr = machine.registers()[A0].to_u64();
                let key = {
                    let key = load_data_h256(machine, key_addr)?;
                    build_raw_key(self.call_context.to_id().unpack(), key.as_slice()).into()
                };
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
                machine.set_register(A0, Mac::REG::from_u64(0));
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
                Ok(true)
            }
            SYS_LOAD_BLOCKINFO => {
                let data = self.block_info.as_slice();
                store_data(machine, data)?;
                Ok(true)
            }
            SYS_LOAD_CALLCONTEXT => {
                let data = self.call_context.as_slice();
                store_data(machine, data)?;
                Ok(true)
            }
            _ => Ok(false),
        }
    }
}
