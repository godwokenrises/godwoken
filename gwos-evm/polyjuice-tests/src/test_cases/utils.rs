use ckb_vm::{
    machine::asm::AsmCoreMachine,
    memory::Memory,
    registers::{A0, A7},
    DefaultMachineBuilder, Error as VMError, Register, SupportMachine, Syscalls,
};
use ckb_vm_aot::AotMachine;
use gw_types::bytes::Bytes;

const BINARY: &[u8] = include_bytes!("../../../build/test_calc_fee");
const DEBUG_PRINT_SYSCALL_NUMBER: u64 = 2177;

pub struct L2Syscalls;

impl<Mac: SupportMachine> Syscalls<Mac> for L2Syscalls {
    fn initialize(&mut self, _machine: &mut Mac) -> Result<(), VMError> {
        Ok(())
    }

    fn ecall(&mut self, machine: &mut Mac) -> Result<bool, VMError> {
        let code = machine.registers()[A7].to_u64();
        if code != DEBUG_PRINT_SYSCALL_NUMBER {
            println!("code: {}", code);
        }
        match code {
            DEBUG_PRINT_SYSCALL_NUMBER => {
                self.output_debug(machine)?;
                Ok(true)
            }
            _ => Ok(false),
        }
    }
}

impl L2Syscalls {
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

        let s = String::from_utf8(buffer)
            .map_err(|_| VMError::Unexpected("Cannot convert to utf-8".to_string()))?;
        println!("[debug]: {}", s);
        Ok(())
    }
}

// TODO: refactor
struct AsmCoreMachineParams {
    pub vm_isa: u8,
    pub vm_version: u32,
}

impl AsmCoreMachineParams {
    pub fn with_version(vm_version: u32) -> Result<AsmCoreMachineParams, VMError> {
        if vm_version == 0 {
            Ok(AsmCoreMachineParams {
                vm_isa: ckb_vm::ISA_IMC,
                vm_version: ckb_vm::machine::VERSION0,
            })
        } else if vm_version == 1 {
            Ok(AsmCoreMachineParams {
                vm_isa: ckb_vm::ISA_IMC | ckb_vm::ISA_B | ckb_vm::ISA_MOP,
                vm_version: ckb_vm::machine::VERSION1,
            })
        } else {
            Err(VMError::InvalidVersion)
        }
    }
}

#[test]
fn test_calc_fee() {
    let binary: Bytes = BINARY.to_vec().into();

    let params = AsmCoreMachineParams::with_version(1).unwrap();
    let cycles = 7000_0000;
    let core_machine = AsmCoreMachine::new(params.vm_isa, params.vm_version, cycles);
    let machine_builder = DefaultMachineBuilder::new(core_machine).syscall(Box::new(L2Syscalls));
    let mut machine = AotMachine::new(machine_builder.build(), None);

    machine.load_program(&binary, &[]).unwrap();
    let code = machine.run().unwrap();
    assert_eq!(code, 0);
}
