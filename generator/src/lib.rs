mod blake2b;
mod smt;
mod syscalls;
#[cfg(test)]
mod tests;

use anyhow::Result;
use blake2b::new_blake2b;
pub use godwoken_types::bytes;
use godwoken_types::packed::{BlockInfo, CallContext};
use lazy_static::lazy_static;
use thiserror::Error;

use crate::bytes::Bytes;
use smt::{Store, H256, SMT};

use syscalls::{L2Syscalls, RunResult};

use ckb_vm::{
    machine::asm::{AsmCoreMachine, AsmMachine},
    DefaultMachineBuilder,
};

lazy_static! {
    static ref VALIDATOR: Bytes = include_bytes!("../../c/build/validator").to_vec().into();
    static ref GENERATOR: Bytes = include_bytes!("../../c/build/generator").to_vec().into();
}

#[derive(Error, Debug, PartialEq, Clone, Eq)]
pub enum Error {
    #[error("invalid exit code {0}")]
    InvalidExitCode(i8),
}

pub struct Context {
    generator: Bytes,
    validator: Bytes,
    block_info: BlockInfo,
    call_context: CallContext,
}

impl Context {
    pub fn new(block_info: BlockInfo, call_context: CallContext) -> Self {
        Context {
            generator: GENERATOR.clone(),
            validator: VALIDATOR.clone(),
            block_info,
            call_context,
        }
    }
}

pub fn execute<S: Store<H256>>(ctx: &Context, tree: &SMT<S>, program: &Bytes) -> Result<RunResult> {
    let mut run_result = RunResult::default();
    {
        let core_machine = Box::<AsmCoreMachine>::default();
        let machine_builder =
            DefaultMachineBuilder::new(core_machine).syscall(Box::new(L2Syscalls {
                tree,
                block_info: &ctx.block_info,
                call_context: &ctx.call_context,
                program: &program,
                result: &mut run_result,
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
