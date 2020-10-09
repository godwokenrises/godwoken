use crate::bytes::Bytes;
use crate::error::Error;
use crate::smt::{Store, H256, SMT};
use crate::syscalls::{L2Syscalls, RunResult};
use godwoken_types::packed::{BlockInfo, CallContext};
use lazy_static::lazy_static;
use std::collections::HashMap;

use ckb_vm::{
    machine::asm::{AsmCoreMachine, AsmMachine},
    DefaultMachineBuilder,
};

lazy_static! {
    static ref VALIDATOR: Bytes = include_bytes!("../../c/build/validator").to_vec().into();
    static ref GENERATOR: Bytes = include_bytes!("../../c/build/generator").to_vec().into();
}

pub struct Context {
    generator: Bytes,
    validator: Bytes,
    block_info: BlockInfo,
    call_context: CallContext,
    contracts_by_code_hash: HashMap<[u8; 32], Bytes>,
}

impl Context {
    pub fn new(
        block_info: BlockInfo,
        call_context: CallContext,
        contracts_by_code_hash: HashMap<[u8; 32], Bytes>,
    ) -> Self {
        Context {
            generator: GENERATOR.clone(),
            validator: VALIDATOR.clone(),
            block_info,
            call_context,
            contracts_by_code_hash,
        }
    }
}

pub fn execute<S: Store<H256>>(ctx: &Context, tree: &SMT<S>) -> Result<RunResult, Error> {
    let mut run_result = RunResult::default();
    {
        let core_machine = Box::<AsmCoreMachine>::default();
        let machine_builder =
            DefaultMachineBuilder::new(core_machine).syscall(Box::new(L2Syscalls {
                tree,
                block_info: &ctx.block_info,
                call_context: &ctx.call_context,
                result: &mut run_result,
                contracts_by_code_hash: &ctx.contracts_by_code_hash,
            }));
        let mut machine = AsmMachine::new(machine_builder.build(), None);
        let program_name = Bytes::from_static(b"generator");
        machine.load_program(&ctx.generator, &[program_name])?;
        let code = machine.run()?;
        if code != 0 {
            return Err(Error::InvalidExitCode(code).into());
        }
    }
    Ok(run_result)
}
