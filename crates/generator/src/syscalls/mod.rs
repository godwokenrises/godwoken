use crate::{
    account_lock_manage::AccountLockManage, backend_manage::BlockConsensus, generator::CyclesPool,
    syscalls::error_codes::GW_FATAL_UNKNOWN_ARGS,
};
use ckb_vm::{
    memory::Memory,
    registers::{A0, A1, A2, A3, A4, A5, A7},
    Error as VMError, Register, SupportMachine, Syscalls,
};
use gw_common::{
    blake2b::new_blake2b,
    registry_address::RegistryAddress,
    state::{
        build_account_field_key, build_data_hash_key, build_script_hash_to_account_id_key, State,
        GW_ACCOUNT_NONCE_TYPE, GW_ACCOUNT_SCRIPT_HASH_TYPE,
    },
};
use gw_config::SyscallCyclesConfig;
use gw_store::state::traits::JournalDB;
use gw_traits::{ChainView, CodeStore};
use gw_types::{
    bytes::Bytes,
    core::ScriptHashType,
    h256::*,
    offchain::CycleMeter,
    packed::{BlockInfo, LogItem, RawL2Transaction, Script},
    prelude::*,
};
use gw_utils::RollupContext;
use std::{cmp, convert::TryInto};

use self::error_codes::{
    GW_ERROR_ACCOUNT_NOT_FOUND, GW_ERROR_DUPLICATED_SCRIPT_HASH, GW_ERROR_INVALID_ACCOUNT_SCRIPT,
    GW_ERROR_NOT_FOUND, GW_ERROR_RECOVER, GW_ERROR_UNKNOWN_SCRIPT_CODE_HASH,
    GW_SUDT_ERROR_UNPERMITTED_ADDRESS, SUCCESS,
};

mod bn;
pub mod error_codes;

/* Constants */
// Increasing from 25k(ethereum contract code size) to 128k.
const MAX_SET_RETURN_DATA_SIZE: u64 = 1024 * 128;

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
/* Syscall for make use the Barreto-Naehrig (BN) curve construction */
const SYS_BN_ADD: u64 = 3601;
const SYS_BN_MUL: u64 = 3602;
const SYS_BN_PAIRING: u64 = 3603;
/* Syscall state revert */
const SYS_SNAPSHOT: u64 = 3701;
const SYS_REVERT: u64 = 3702;
/* Syscall permissions */
const SYS_CHECK_SUDT_ADDRESS: u64 = 3801;
/* CKB compatible syscalls */
const DEBUG_PRINT_SYSCALL_NUMBER: u64 = 2177;

#[derive(Debug, Default)]
pub struct RunContext {
    pub cycle_meter: CycleMeter,
    pub return_data: Bytes,
    pub exit_code: i8,
    pub debug_log_buf: Vec<u8>,
}

impl RunContext {
    pub fn is_success(&self) -> bool {
        self.exit_code == 0
    }
}

pub(crate) struct L2Syscalls<'a, 'b, S, C> {
    pub(crate) chain: &'a C,
    pub(crate) state: &'a mut S,
    pub(crate) rollup_context: &'a RollupContext,
    pub(crate) account_lock_manage: &'a AccountLockManage,
    pub(crate) block_info: &'a BlockInfo,
    pub(crate) block_consensus: &'a BlockConsensus,
    pub(crate) raw_tx: &'a RawL2Transaction,
    pub(crate) context: &'b mut RunContext,
    pub(crate) cycles_pool: &'b mut Option<&'a mut CyclesPool>,
}

#[allow(dead_code)]
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
    Ok(data)
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

impl<'a, 'b, S: State + CodeStore + JournalDB, C: ChainView, Mac: SupportMachine> Syscalls<Mac>
    for L2Syscalls<'a, 'b, S, C>
{
    fn initialize(&mut self, _machine: &mut Mac) -> Result<(), VMError> {
        Ok(())
    }

    fn ecall(&mut self, machine: &mut Mac) -> Result<bool, VMError> {
        let code = machine.registers()[A7].to_u64();

        if let Some(cycles_pool) = self.cycles_pool {
            let syscall_cycles = Self::get_syscall_cycles(code, cycles_pool.syscall_config());
            if 0 != syscall_cycles {
                self.context.cycle_meter.r#virtual = self
                    .context
                    .cycle_meter
                    .r#virtual
                    .saturating_add(syscall_cycles);

                // Subtract cycles to interrupt execution eariler
                let execution_and_virtual = machine
                    .cycles()
                    .saturating_add(self.context.cycle_meter.r#virtual);
                if cycles_pool.consume_cycles(syscall_cycles).is_none()
                    || execution_and_virtual > cycles_pool.limit()
                {
                    return Err(VMError::Unexpected("cycles pool limit reached".to_owned()));
                }
            }
        }

        match code {
            SYS_STORE => {
                let key_addr = machine.registers()[A0].to_u64();
                let key = load_data_h256(machine, key_addr)?;
                let value_addr = machine.registers()[A1].to_u64();
                let value = load_data_h256(machine, value_addr)?;
                self.state
                    .update_raw(key, value)
                    .map_err(|err| VMError::Unexpected(format!("store kv error: {}", err)))?;
                machine.set_register(A0, Mac::REG::from_u8(SUCCESS));
                Ok(true)
            }
            SYS_LOAD => {
                let key_addr = machine.registers()[A0].to_u64();
                let key = load_data_h256(machine, key_addr)?;
                let value_addr = machine.registers()[A1].to_u64();
                let value = self
                    .state
                    .get_raw(&key)
                    .map_err(|err| VMError::Unexpected(format!("get raw: {}", err)))?;
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
                    return Err(VMError::Unexpected("exceeded max return data".to_owned()));
                }
                let data = load_bytes(machine, data_addr, len as usize)?;
                self.context.return_data = data.into();
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
                if self.get_account_id_by_script_hash(&script_hash)?.is_some() {
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
                        .any(|type_hash| type_hash.hash() == script.code_hash());
                    let is_contract_account = self
                        .rollup_context
                        .rollup_config
                        .allowed_contract_type_hashes()
                        .into_iter()
                        .any(|type_hash| type_hash.hash() == script.code_hash());
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
                let id = self
                    .state
                    .get_account_count()
                    .map_err(|err| VMError::Unexpected(format!("get account count: {}", err)))?;
                self.state
                    .update_raw(
                        build_account_field_key(id, GW_ACCOUNT_NONCE_TYPE),
                        H256::zero(),
                    )
                    .map_err(|err| VMError::Unexpected(format!("store nonce error: {}", err)))?;
                self.state
                    .update_raw(
                        build_account_field_key(id, GW_ACCOUNT_SCRIPT_HASH_TYPE),
                        script_hash,
                    )
                    .map_err(|err| {
                        VMError::Unexpected(format!("store script hash error: {}", err))
                    })?;
                // script hash to id
                let script_hash_to_id_value: H256 = {
                    let mut buf: [u8; 32] = H256::from_u32(id);
                    // the first 4 bytes is id, set exists flag(fifth byte) to 1
                    buf[4] = 1;
                    buf
                };
                self.state
                    .update_raw(
                        build_script_hash_to_account_id_key(&script_hash[..]),
                        script_hash_to_id_value,
                    )
                    .map_err(|err| {
                        VMError::Unexpected(format!("store script hash to id error: {}", err))
                    })?;
                // insert script
                self.state.insert_script(script_hash, script);
                self.state
                    .set_account_count(id + 1)
                    .map_err(|err| VMError::Unexpected(format!("set acccount: {}", err)))?;
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
                    let err_msg =
                        format!("syscall error: get script hash by account id: {:?}", err);
                    log::error!("{}", err_msg);
                    VMError::Unexpected(err_msg)
                })?;
                // return not found if script_hash is zero, otherwise we search the script from DB
                if script_hash.is_zero() {
                    machine.set_register(A0, Mac::REG::from_i8(GW_ERROR_ACCOUNT_NOT_FOUND));
                    return Ok(true);
                }
                let script = self.state.get_script(&script_hash).ok_or_else(|| {
                    let err_msg = format!(
                        "syscall error: script not found by script hash: {:?}",
                        script_hash
                    );
                    log::error!("{}", err_msg);
                    VMError::Unexpected(err_msg)
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
                self.state
                    .update_raw(data_hash_key, H256::one())
                    .map_err(|err| VMError::Unexpected(format!("store data: {}", err)))?;
                // write data
                self.state.insert_data(data_hash, data.into());
                machine.set_register(A0, Mac::REG::from_u8(SUCCESS));
                Ok(true)
            }
            SYS_LOAD_DATA => {
                let data_hash_addr = machine.registers()[A3].to_u64();
                let data_hash = load_data_h256(machine, data_hash_addr)?;
                let data = match self.state.get_data(&data_hash) {
                    Some(data) => data,
                    None => {
                        machine.set_register(A0, Mac::REG::from_i8(GW_ERROR_NOT_FOUND));
                        return Ok(true);
                    }
                };
                store_data(machine, data.as_ref())?;
                machine.set_register(A0, Mac::REG::from_u8(SUCCESS));
                Ok(true)
            }
            SYS_GET_BLOCK_HASH => {
                let block_hash_addr = machine.registers()[A0].to_u64();
                let number = machine.registers()[A1].to_u64();

                let block_hash_opt =
                    self.chain.get_block_hash_by_number(number).map_err(|err| {
                        let err_msg = format!(
                            "syscall error: get block hash by number: {}, error: {:?}",
                            number, err
                        );
                        log::error!("{}", err_msg);
                        VMError::Unexpected(err_msg)
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
                self.state.append_log(
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

                // fetch payer address
                let payer_addr = {
                    let payer_addr = machine.registers()[A0].to_u64();
                    let payer_addr_len = machine.registers()[A1].to_u64();
                    // addr len: 4 registry id + 4 addr len + 20 addr
                    if payer_addr_len != 28u64 {
                        let err_msg =
                            format!("unexpected payer address length: {}", payer_addr_len);
                        log::error!("{}", err_msg);
                        return Err(VMError::Unexpected(err_msg));
                    }
                    let payer_addr_bytes =
                        load_bytes(machine, payer_addr, payer_addr_len as usize)?;
                    match RegistryAddress::from_slice(&payer_addr_bytes) {
                        Some(addr) => addr,
                        None => {
                            log::error!("invalid payer address");
                            return Err(VMError::Unexpected("invalid payer address".to_owned()));
                        }
                    }
                };
                let sudt_id = machine.registers()[A2].to_u8();
                let amount = {
                    let amount_addr = machine.registers()[A3].to_u64();
                    load_data_h256(machine, amount_addr)?.to_u256()
                };

                // TODO record fee payment in the generator context
                log::debug!(
                    "[contract syscall: SYS_PAY_FEE] payer: {}, registry_id: {}, sudt_id: {}, amount: {}",
                    hex::encode(&payer_addr.address),
                    payer_addr.registry_id,
                    sudt_id,
                    amount
                );
                machine.set_register(A0, Mac::REG::from_u8(SUCCESS));
                Ok(true)
            }
            SYS_BN_ADD => {
                let input_addr = machine.registers()[A3].to_u64();
                let input_size = machine.registers()[A4].to_u64();
                let input = load_bytes(machine, input_addr, input_size as usize)?;

                let output = bn::add(&input).map_err(|err| {
                    let err_msg = format!("syscall SYS_BN_ADD error: {:?}", err.0);
                    log::error!("{}", err_msg);
                    VMError::Unexpected(err_msg)
                })?;
                store_data(machine, output.as_slice())?;

                machine.set_register(A0, Mac::REG::from_u8(SUCCESS));
                Ok(true)
            }
            SYS_BN_MUL => {
                let input_addr = machine.registers()[A3].to_u64();
                let input_size = machine.registers()[A4].to_u64();
                let input = load_bytes(machine, input_addr, input_size as usize)?;

                let output = bn::mul(&input).map_err(|err| {
                    let err_msg = format!("syscall SYS_BN_MUL error: {:?}", err.0);
                    log::error!("{}", err_msg);
                    VMError::Unexpected(err_msg)
                })?;
                store_data(machine, output.as_slice())?;

                machine.set_register(A0, Mac::REG::from_u8(SUCCESS));
                Ok(true)
            }
            SYS_BN_PAIRING => {
                let input_addr = machine.registers()[A3].to_u64();
                let input_size = machine.registers()[A4].to_u64();
                let input = load_bytes(machine, input_addr, input_size as usize)?;

                if let Some(cycles_pool) = self.cycles_pool {
                    // k is the number of pairings being computed
                    let k: u64 = input_size / 192;
                    if k > 0 {
                        // Subtract additional cycles per pairing
                        let additional_cycles =
                            k * cycles_pool.syscall_config().sys_bn_per_pairing_cycles;
                        self.context.cycle_meter.r#virtual = self
                            .context
                            .cycle_meter
                            .r#virtual
                            .saturating_add(additional_cycles);
                        let execution_and_virtual = machine
                            .cycles()
                            .saturating_add(self.context.cycle_meter.r#virtual);
                        if cycles_pool.consume_cycles(additional_cycles).is_none()
                            || execution_and_virtual > cycles_pool.limit()
                        {
                            return Err(VMError::Unexpected(
                                "cycles pool limit reached".to_owned(),
                            ));
                        }
                    }
                }

                let output = bn::pairing(&input).map_err(|err| {
                    let err_msg = format!("syscall SYS_BN_PAIRING error: {:?}", err.0);
                    log::error!("{}", err_msg);
                    VMError::Unexpected(err_msg)
                })?;
                store_data(machine, output.as_slice())?;
                machine.set_register(A0, Mac::REG::from_u8(SUCCESS));
                Ok(true)
            }
            SYS_SNAPSHOT => {
                let snapshot_addr = machine.registers()[A0].clone();
                // create snapshot
                let snapshot_id = self.state.snapshot() as u32;
                machine
                    .memory_mut()
                    .store32(&snapshot_addr, &Mac::REG::from_u32(snapshot_id))?;
                machine.set_register(A0, Mac::REG::from_u8(SUCCESS));
                Ok(true)
            }
            SYS_REVERT => {
                let snapshot_id = machine.registers()[A0].to_u32();
                self.state
                    .revert(snapshot_id as usize)
                    .map_err(|err| VMError::Unexpected(format!("revert: {}", err)))?;
                machine.set_register(A0, Mac::REG::from_u8(SUCCESS));
                Ok(true)
            }
            SYS_CHECK_SUDT_ADDRESS => {
                let sudt_proxy_addr = machine.registers()[A0].to_u64();
                let address: [u8; 20] = load_bytes(machine, sudt_proxy_addr, 20)?
                    .try_into()
                    .unwrap();
                let is_permit = self
                    .block_consensus
                    .sudt_proxy
                    .address_list
                    .contains(&address);
                if self
                    .block_consensus
                    .sudt_proxy
                    .permit_sudt_transfer_from_dangerous_contract
                    || is_permit
                {
                    machine.set_register(A0, Mac::REG::from_u8(SUCCESS));
                } else {
                    machine.set_register(A0, Mac::REG::from_i8(GW_SUDT_ERROR_UNPERMITTED_ADDRESS));
                }
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

impl<'a, 'b, S: State, C: ChainView> L2Syscalls<'a, 'b, S, C> {
    fn get_script_hash(&mut self, id: u32) -> Result<H256, VMError> {
        let value = self
            .state
            .get_raw(&build_account_field_key(id, GW_ACCOUNT_SCRIPT_HASH_TYPE))
            .map_err(|err| {
                let err_msg = format!("syscall error: get script hash by account id : {:?}", err);
                log::error!("{}", err);
                VMError::Unexpected(err_msg)
            })?;
        Ok(value)
    }
    fn get_account_id_by_script_hash(
        &mut self,
        script_hash: &H256,
    ) -> Result<Option<u32>, VMError> {
        let value = self
            .state
            .get_raw(&build_script_hash_to_account_id_key(script_hash.as_slice()))
            .map_err(|err| {
                let err_msg = format!("syscall error: get account id by script hash : {:?}", err);
                log::error!("{}", err_msg);
                VMError::Unexpected(err_msg)
            })?;
        if value.is_zero() {
            return Ok(None);
        }
        let id = value.to_u32();
        Ok(Some(id))
    }

    fn output_debug<Mac: SupportMachine>(&mut self, machine: &mut Mac) -> Result<(), VMError> {
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
        self.context.debug_log_buf.push(b'\n');
        self.context.debug_log_buf.extend_from_slice(&buffer);
        Ok(())
    }

    fn get_syscall_cycles(syscall: u64, cycles_config: &SyscallCyclesConfig) -> u64 {
        match syscall {
            SYS_STORE => cycles_config.sys_store_cycles,
            SYS_LOAD => cycles_config.sys_load_cycles,
            SYS_CREATE => cycles_config.sys_create_cycles,
            SYS_LOAD_ACCOUNT_SCRIPT => cycles_config.sys_load_account_script_cycles,
            SYS_STORE_DATA => cycles_config.sys_store_data_cycles,
            SYS_LOAD_DATA => cycles_config.sys_load_data_cycles,
            SYS_GET_BLOCK_HASH => cycles_config.sys_get_block_hash_cycles,
            SYS_RECOVER_ACCOUNT => cycles_config.sys_recover_account_cycles,
            SYS_LOG => cycles_config.sys_log_cycles,
            SYS_BN_ADD => cycles_config.sys_bn_add_cycles,
            SYS_BN_MUL => cycles_config.sys_bn_mul_cycles,
            SYS_BN_PAIRING => cycles_config.sys_bn_fixed_pairing_cycles,
            SYS_SNAPSHOT => cycles_config.sys_snapshot_cycles,
            SYS_REVERT => cycles_config.sys_revert_cycles,
            _ => 0,
        }
    }
}
