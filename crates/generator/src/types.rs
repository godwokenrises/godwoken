use gw_types::packed::{ChallengeTarget, ChallengeWitness};
use std::fmt::{self, Display};

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
