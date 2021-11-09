use ckb_vm::{
    machine::{VERSION0, VERSION1},
    ISA_B, ISA_IMC, ISA_MOP,
};
use gw_types::packed::{ChallengeTarget, ChallengeWitness};
use std::fmt::{self, Display};

#[cfg(has_asm)]
use ckb_vm::machine::asm::AsmCoreMachine;

#[cfg(not(has_asm))]
use ckb_vm::{DefaultCoreMachine, SparseMemory, TraceMachine, WXorXMemory};

/// The type of CKB-VM ISA.
pub type VmIsa = u8;
/// /// The type of CKB-VM version.
pub type VmVersion = u32;

#[cfg(has_asm)]
pub(crate) type CoreMachineType = AsmCoreMachine;
#[cfg(not(has_asm))]
pub(crate) type CoreMachineType = DefaultCoreMachine<u64, WXorXMemory<SparseMemory<u64>>>;

/// The type of core VM machine when uses ASM.
#[cfg(has_asm)]
pub type CoreMachine = Box<AsmCoreMachine>;
/// The type of core VM machine when doesn't use ASM.
#[cfg(not(has_asm))]
pub type CoreMachine = DefaultCoreMachine<u64, WXorXMemory<SparseMemory<u64>>>;

/// The version of CKB VM.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum VMVersion {
    /// CKB VM 0 with Syscall version 1.
    V0 = 0,
    /// CKB VM 1 with Syscall version 1 and version 2.
    V1 = 1,
}

impl VMVersion {
    /// Returns the latest version.
    pub const fn latest() -> Self {
        Self::V1
    }

    /// Returns the ISA set of CKB VM in current script version.
    pub fn vm_isa(self) -> VmIsa {
        match self {
            Self::V0 => ISA_IMC,
            Self::V1 => ISA_IMC | ISA_B | ISA_MOP,
        }
    }

    /// Returns the version of CKB VM in current script version.
    pub fn vm_version(self) -> VmVersion {
        match self {
            Self::V0 => VERSION0,
            Self::V1 => VERSION1,
        }
    }

    /// Creates a CKB VM core machine without cycles limit.
    ///
    /// In fact, there is still a limit of `max_cycles` which is set to `2^64-1`.
    pub fn init_core_machine_without_limit(self) -> CoreMachine {
        self.init_core_machine(u64::MAX)
    }

    /// Creates a CKB VM core machine.
    pub fn init_core_machine(self, max_cycles: u64) -> CoreMachine {
        let isa = self.vm_isa();
        let version = self.vm_version();
        CoreMachineType::new(isa, version, max_cycles)
    }
}

// #[cfg(has_asm)]
// pub(crate) type Machine<'a> = AsmMachine<'a>;
#[cfg(not(has_asm))]
pub(crate) type Machine<'a> = TraceMachine<'a, CoreMachine>;

#[derive(Debug, Clone, Eq, PartialEq)]
pub struct ChallengeContext {
    pub target: ChallengeTarget,
    pub witness: ChallengeWitness,
}

impl Display for ChallengeContext {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{{target: {}, witness: {}}}", self.target, self.witness)
    }
}
