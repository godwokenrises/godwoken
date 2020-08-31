mod blake2b;
mod smt;
mod syscalls;

use anyhow::Result;
pub use godwoken_types::bytes;
use godwoken_types::packed::{BlockInfo, CallContext};
use thiserror::Error;

use crate::bytes::Bytes;
use smt::{Store, H256, SMT};

use syscalls::{L2Syscalls, RunResult};

use ckb_vm::{
    machine::asm::{AsmCoreMachine, AsmMachine},
    DefaultMachineBuilder,
};

#[derive(Error, Debug, PartialEq, Clone, Eq)]
pub enum Error {
    #[error("invalid exit code {}", "_0")]
    InvalidExitCode(i8),
}

pub struct Context {
    generator: Bytes,
    validator: Bytes,
    block_info: BlockInfo,
    call_context: CallContext,
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
                result: &mut run_result,
            }));
        let mut machine = AsmMachine::new(machine_builder.build(), None);
        let program_name = Bytes::from_static(b"generator");
        let program_length_bytes = (program.len() as u32).to_le_bytes()[..].to_vec();
        let program_length = Bytes::from(program_length_bytes);
        machine.load_program(
            &ctx.generator,
            &[program_name, program_length, program.clone()],
        )?;
        let code = machine.run()?;
        if code != 0 {
            return Err(Error::InvalidExitCode(code).into());
        }
    }
    Ok(run_result)
}
