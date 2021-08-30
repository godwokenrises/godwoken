use crate::{account_lock_manage::AccountLockManage, syscalls::error_codes::GW_FATAL_UNKNOWN_ARGS};
use ckb_vm::{
    memory::Memory,
    registers::{A0, A1, A2, A3, A4, A5, A7},
    Error as VMError, Register, SupportMachine, Syscalls,
};
use gw_common::{
    blake2b::new_blake2b,
    h256_ext::H256Ext,
    state::{
        build_account_field_key, build_data_hash_key, build_script_hash_to_account_id_key,
        build_short_script_hash_to_script_hash_key, State, DEFAULT_SHORT_SCRIPT_HASH_LEN,
        GW_ACCOUNT_NONCE_TYPE, GW_ACCOUNT_SCRIPT_HASH_TYPE,
    },
    H256,
};
use gw_traits::{ChainStore, CodeStore};
use gw_types::{
    bytes::Bytes,
    core::ScriptHashType,
    offchain::{RollupContext, RunResult},
    packed::{BlockInfo, LogItem, RawL2Transaction, Script},
    prelude::*,
};
use std::cmp;

use self::error_codes::{
    GW_ERROR_ACCOUNT_NOT_FOUND, GW_ERROR_DUPLICATED_SCRIPT_HASH, GW_ERROR_INVALID_ACCOUNT_SCRIPT,
    GW_ERROR_NOT_FOUND, GW_ERROR_RECOVER, GW_ERROR_UNKNOWN_SCRIPT_CODE_HASH, SUCCESS,
};

pub mod error_codes;

/* Constants */
// 24KB is max ethereum contract code size
const MAX_SET_RETURN_DATA_SIZE: u64 = 1024 * 24;

/* Syscall account store / load / create */
const SYS_CREATE: u64 = 3100;
const SYS_STORE: u64 = 3101;
const SYS_LOAD: u64 = 3102;
const SYS_LOAD_ACCOUNT_SCRIPT: u64 = 3105;
/* Syscall call / return */
const SYS_SET_RETURN_DATA: u64 = 3201;
/* Syscall data store / load */
const SYS_STORE_DATA: u64 = 3301;
const SYS_LOAD_DATA: u64 = 3302;
/* Syscall load metadata structures */
const SYS_LOAD_ROLLUP_CONFIG: u64 = 3401;
const SYS_LOAD_TRANSACTION: u64 = 3402;
const SYS_LOAD_BLOCKINFO: u64 = 3403;
const SYS_GET_BLOCK_HASH: u64 = 3404;
/* Syscall builtins */
const SYS_PAY_FEE: u64 = 3501;
const SYS_LOG: u64 = 3502;
const SYS_RECOVER_ACCOUNT: u64 = 3503;
/* CKB compatible syscalls */
const DEBUG_PRINT_SYSCALL_NUMBER: u64 = 2177;

pub(crate) struct L2Syscalls<'a, S, C> {
    pub(crate) chain: &'a C,
    pub(crate) state: &'a S,
    pub(crate) rollup_context: &'a RollupContext,
    pub(crate) account_lock_manage: &'a AccountLockManage,
    pub(crate) block_info: &'a BlockInfo,
    pub(crate) raw_tx: &'a RawL2Transaction,
    pub(crate) code_store: &'a dyn CodeStore,
    pub(crate) result: &'a mut RunResult,
}

fn load_data_u128<Mac: SupportMachine>(machine: &mut Mac, addr: u64) -> Result<u128, VMError> {
    let mut data = [0u8; 16];
    for (i, c) in data.iter_mut().enumerate() {
        *c = machine
            .memory_mut()
            .load8(&Mac::REG::from_u64(addr).overflowing_add(&Mac::REG::from_u64(i as u64)))?
            .to_u8();
    }
    Ok(u128::from_le_bytes(data))
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

#[allow(clippy::needless_range_loop)]
fn load_bytes<Mac: SupportMachine>(
    machine: &mut Mac,
    addr: u64,
    len: usize,
) -> Result<Vec<u8>, VMError> {
    let mut data = vec![0; len];
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

impl<'a, S: State, C: ChainStore, Mac: SupportMachine> Syscalls<Mac> for L2Syscalls<'a, S, C> {
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
            SYS_CREATE => {
                let script_addr = machine.registers()[A0].to_u64();
                let script_len = machine.registers()[A1].to_u64();
                let account_id_addr = machine.registers()[A2].clone();

                let script_data = load_bytes(machine, script_addr, script_len as usize)?;
                let script = match Script::from_slice(&script_data[..]) {
                    Ok(script) => script,
                    Err(err) => {
                        log::error!("syscall error: invalid script to create : {:?}", err);
                        machine
                            .set_register(A0, Mac::REG::from_i8(GW_ERROR_INVALID_ACCOUNT_SCRIPT));
                        return Ok(true);
                    }
                };
                let script_hash = script.hash();

                // Return error if script_hash is exists
                if self
                    .get_account_id_by_script_hash(&script_hash.into())?
                    .is_some()
                {
                    machine.set_register(A0, Mac::REG::from_i8(GW_ERROR_DUPLICATED_SCRIPT_HASH));
                    return Ok(true);
                }

                // Check script validity
                {
                    // check hash type
                    if script.hash_type() != ScriptHashType::Type.into() {
                        log::debug!("Invalid deposit account script: unexpected hash_type: Data");
                        machine
                            .set_register(A0, Mac::REG::from_i8(GW_ERROR_UNKNOWN_SCRIPT_CODE_HASH));
                        return Ok(true);
                    }

                    // check code hash
                    let is_eoa_account = self
                        .rollup_context
                        .rollup_config
                        .allowed_eoa_type_hashes()
                        .into_iter()
                        .any(|type_hash| type_hash == script.code_hash());
                    let is_contract_account = self
                        .rollup_context
                        .rollup_config
                        .allowed_contract_type_hashes()
                        .into_iter()
                        .any(|type_hash| type_hash == script.code_hash());
                    if !is_eoa_account && !is_contract_account {
                        log::debug!(
                            "Invalid deposit account script: unknown code_hash: {:?}",
                            hex::encode(script.code_hash().as_slice())
                        );
                        machine
                            .set_register(A0, Mac::REG::from_i8(GW_ERROR_UNKNOWN_SCRIPT_CODE_HASH));
                        return Ok(true);
                    }

                    // check args
                    let args: Bytes = script.args().unpack();
                    if args.len() < 32 {
                        log::debug!(
                            "Invalid deposit account args, expect len: 32, got: {}",
                            args.len()
                        );
                        machine
                            .set_register(A0, Mac::REG::from_i8(GW_ERROR_INVALID_ACCOUNT_SCRIPT));
                        return Ok(true);
                    }
                    if &args[..32] != self.rollup_context.rollup_script_hash.as_slice() {
                        log::debug!(
                            "Invalid deposit account args, expect rollup_script_hash: {}, got: {}",
                            hex::encode(self.rollup_context.rollup_script_hash.as_slice()),
                            hex::encode(&args[..32])
                        );

                        machine
                            .set_register(A0, Mac::REG::from_i8(GW_ERROR_INVALID_ACCOUNT_SCRIPT));
                        return Ok(true);
                    }
                }

                // Same logic from State::create_account()
                let id = self.get_account_count()?;
                self.result.write_values.insert(
                    build_account_field_key(id, GW_ACCOUNT_NONCE_TYPE),
                    H256::zero(),
                );
                self.result.write_values.insert(
                    build_account_field_key(id, GW_ACCOUNT_SCRIPT_HASH_TYPE),
                    script_hash.into(),
                );
                // script hash to id
                self.result.write_values.insert(
                    build_script_hash_to_account_id_key(&script_hash[..]),
                    H256::from_u32(id),
                );
                // short script hash to script_hash
                self.result.write_values.insert(
                    build_short_script_hash_to_script_hash_key(
                        &script_hash[..DEFAULT_SHORT_SCRIPT_HASH_LEN],
                    ),
                    script_hash.into(),
                );
                // insert script
                self.result
                    .new_scripts
                    .insert(script_hash.into(), script.as_slice().to_vec());
                self.set_account_count(id + 1);
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
            SYS_LOAD_ACCOUNT_SCRIPT => {
                let account_id = machine.registers()[A3].to_u32();
                let script_hash = self.get_script_hash(account_id).map_err(|err| {
                    log::error!("syscall error: get script hash by account id: {:?}", err);
                    VMError::Unexpected
                })?;
                // return not found if script_hash is zero, otherwise we search the script from DB
                if script_hash.is_zero() {
                    machine.set_register(A0, Mac::REG::from_i8(GW_ERROR_ACCOUNT_NOT_FOUND));
                    return Ok(true);
                }
                let script = self.get_script(&script_hash).ok_or_else(|| {
                    log::error!(
                        "syscall error: script not found by script hash: {:?}",
                        script_hash
                    );
                    VMError::Unexpected
                })?;
                let data = script.as_slice();
                store_data(machine, data)?;
                machine.set_register(A0, Mac::REG::from_u8(SUCCESS));
                Ok(true)
            }
            SYS_STORE_DATA => {
                let data_len = machine.registers()[A0].to_u64();
                let data_addr = machine.registers()[A1].to_u64();

                let data = load_bytes(machine, data_addr, data_len as usize)?;
                let mut data_hash = [0u8; 32];
                let mut hasher = new_blake2b();
                hasher.update(data.as_ref());
                hasher.finalize(&mut data_hash);
                // insert data hash into SMT
                let data_hash_key = build_data_hash_key(&data_hash);
                self.result.write_values.insert(data_hash_key, H256::one());
                // write data
                self.result
                    .write_data
                    .insert(data_hash.into(), data.as_slice().to_vec());
                machine.set_register(A0, Mac::REG::from_u8(SUCCESS));
                Ok(true)
            }
            SYS_LOAD_DATA => {
                let data_hash_addr = machine.registers()[A3].to_u64();
                let data_hash = load_data_h256(machine, data_hash_addr)?;
                let data = match self.get_data(&data_hash) {
                    Some(data) => data,
                    None => {
                        machine.set_register(A0, Mac::REG::from_i8(GW_ERROR_NOT_FOUND));
                        return Ok(true);
                    }
                };
                store_data(machine, data.as_ref())?;
                self.result.read_data.insert(data_hash, data.to_vec());
                machine.set_register(A0, Mac::REG::from_u8(SUCCESS));
                Ok(true)
            }
            SYS_GET_BLOCK_HASH => {
                let block_hash_addr = machine.registers()[A0].to_u64();
                let number = machine.registers()[A1].to_u64();

                let block_hash_opt =
                    self.chain.get_block_hash_by_number(number).map_err(|err| {
                        log::error!(
                            "syscall error: get block hash by number: {}, error: {:?}",
                            number,
                            err
                        );
                        VMError::Unexpected
                    })?;
                if let Some(hash) = block_hash_opt {
                    machine
                        .memory_mut()
                        .store_bytes(block_hash_addr, hash.as_slice())?;
                    machine.set_register(A0, Mac::REG::from_u8(SUCCESS));
                } else {
                    // Can not get block hash by number
                    machine.set_register(A0, Mac::REG::from_u8(0xff));
                }
                Ok(true)
            }
            SYS_RECOVER_ACCOUNT => {
                // gw_recover_account(msg: Byte32, signature: Bytes, code_hash: Byte32) -> Script
                let script_addr = machine.registers()[A0].to_u64();
                let script_len_addr = machine.registers()[A1].clone();
                let msg_addr = machine.registers()[A2].to_u64();
                let signature_addr = machine.registers()[A3].to_u64();
                let signature_len = machine.registers()[A4].to_u64();
                let code_hash_addr = machine.registers()[A5].to_u64();

                let msg = load_data_h256(machine, msg_addr)?;
                let signature = load_bytes(machine, signature_addr, signature_len as usize)?;
                let code_hash = load_data_h256(machine, code_hash_addr)?;

                if let Some(lock_algo) = self.account_lock_manage.get_lock_algorithm(&code_hash) {
                    if let Ok(lock_args) = lock_algo.recover(msg, &signature) {
                        let mut script_args = vec![0u8; 32 + lock_args.len()];
                        script_args[0..32]
                            .copy_from_slice(self.rollup_context.rollup_script_hash.as_slice());
                        script_args[32..32 + lock_args.len()].copy_from_slice(lock_args.as_ref());
                        let account_script = Script::new_builder()
                            .code_hash(code_hash.pack())
                            .hash_type(ScriptHashType::Type.into())
                            .args(Bytes::from(script_args).pack())
                            .build();

                        machine.memory_mut().store64(
                            &script_len_addr,
                            &Mac::REG::from_u64(account_script.as_slice().len() as u64),
                        )?;
                        machine
                            .memory_mut()
                            .store_bytes(script_addr, account_script.as_slice())?;
                        machine.set_register(A0, Mac::REG::from_u8(SUCCESS));
                    } else {
                        machine.set_register(A0, Mac::REG::from_i8(GW_ERROR_RECOVER));
                    }
                } else {
                    log::debug!("unexpected lock code hash: {:?}", code_hash);
                    machine.set_register(A0, Mac::REG::from_i8(GW_FATAL_UNKNOWN_ARGS));
                }

                Ok(true)
            }
            SYS_LOG => {
                let account_id = machine.registers()[A0].to_u32();
                let service_flag = machine.registers()[A1].to_u8();
                let data_len = machine.registers()[A2].to_u64();
                let data_addr = machine.registers()[A3].to_u64();

                let data = load_bytes(machine, data_addr, data_len as usize)?;
                self.result.logs.push(
                    LogItem::new_builder()
                        .account_id(account_id.pack())
                        .service_flag(service_flag.into())
                        .data(Bytes::from(data).pack())
                        .build(),
                );
                machine.set_register(A0, Mac::REG::from_u8(SUCCESS));
                Ok(true)
            }
            SYS_LOAD_ROLLUP_CONFIG => {
                let data = self.rollup_context.rollup_config.as_slice();
                store_data(machine, data)?;
                machine.set_register(A0, Mac::REG::from_u8(SUCCESS));
                Ok(true)
            }
            SYS_PAY_FEE => {
                // record the fee to block producer
                // NOTICE this syscall do not actually execute the transfer of assets,
                // the trusted script should do the transfer of fee first,
                // then called this syscal to record the fee only after the success of the transfer.

                // fetch short script hash
                let short_address = {
                    let short_address_addr = machine.registers()[A0].to_u64();
                    let short_address_len = machine.registers()[A1].to_u64();
                    // check short address len
                    if short_address_len != DEFAULT_SHORT_SCRIPT_HASH_LEN as u64 {
                        log::error!("unexpected script hash short length: {}", short_address_len);
                        return Err(VMError::Unexpected);
                    }
                    load_bytes(machine, short_address_addr, short_address_len as usize)?
                };
                let sudt_id = machine.registers()[A2].to_u8();
                let amount = {
                    let amount_addr = machine.registers()[A3].to_u64();
                    load_data_u128(machine, amount_addr)?
                };

                // TODO record fee payment in the generator context
                log::debug!(
                    "[contract syscall: SYS_PAY_FEE] payer: {}, sudt_id: {}, amount: {}",
                    hex::encode(&short_address),
                    sudt_id,
                    amount
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

impl<'a, S: State, C: ChainStore> L2Syscalls<'a, S, C> {
    fn get_raw(&mut self, key: &H256) -> Result<H256, VMError> {
        let value = match self.result.write_values.get(key) {
            Some(value) => *value,
            None => {
                let tree_value = self.state.get_raw(key).map_err(|_| VMError::Unexpected)?;
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
                log::error!("syscall error: get account count : {:?}", err);
                VMError::Unexpected
            })
        }
    }
    fn set_account_count(&mut self, count: u32) {
        self.result.account_count = Some(count);
    }
    fn get_script(&mut self, script_hash: &H256) -> Option<Script> {
        let opt_script = self
            .result
            .new_scripts
            .get(script_hash)
            .map(|data| Script::from_slice(data).expect("Script"))
            .or_else(|| self.code_store.get_script(script_hash));

        if let Some(ref script) = opt_script {
            self.result.get_scripts.insert(script.as_slice().to_vec());
        }

        opt_script
    }
    fn get_data(&self, data_hash: &H256) -> Option<Bytes> {
        self.result
            .write_data
            .get(data_hash)
            .map(|data| Bytes::from(data.clone()))
            .or_else(|| self.code_store.get_data(data_hash))
    }
    fn get_script_hash(&mut self, id: u32) -> Result<H256, VMError> {
        let value = self
            .get_raw(&build_account_field_key(id, GW_ACCOUNT_SCRIPT_HASH_TYPE))
            .map_err(|err| {
                log::error!("syscall error: get script hash by account id : {:?}", err);
                VMError::Unexpected
            })?;
        Ok(value)
    }
    fn get_account_id_by_script_hash(
        &mut self,
        script_hash: &H256,
    ) -> Result<Option<u32>, VMError> {
        let value = self
            .get_raw(&build_script_hash_to_account_id_key(script_hash.as_slice()))
            .map_err(|err| {
                log::error!("syscall error: get account id by script hash : {:?}", err);
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
        log::debug!("[contract debug]: {}", s);
        Ok(())
    }
}
