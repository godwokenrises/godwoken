use crate::bindings::{
    smt_calculate_root, smt_pair_t, smt_state_fetch, smt_state_init, smt_state_insert,
    smt_state_normalize, smt_state_t, smt_verify, SMTErrorCode,
};
pub type Pair = smt_pair_t;

pub struct Tree<'a> {
    _buf: &'a mut [Pair],
    state: smt_state_t,
}

impl<'a> Tree<'a> {
    pub fn new(buf: &'a mut [Pair]) -> Tree<'a> {
        let state = unsafe {
            let mut state = core::mem::MaybeUninit::uninit();
            smt_state_init(state.as_mut_ptr(), buf.as_mut_ptr(), buf.len() as u32);
            state.assume_init()
        };
        Self { _buf: buf, state }
    }

    pub fn update(&mut self, key: &[u8; 32], value: &[u8; 32]) -> Result<(), SMTErrorCode> {
        match unsafe { smt_state_insert(&mut self.state, key.as_ptr(), value.as_ptr()) } {
            0 => Ok(()),
            err => Err(err as u32),
        }
    }

    pub fn get(&self, key: &[u8; 32]) -> Result<[u8; 32], SMTErrorCode> {
        let mut value = [0u8; 32];
        match unsafe { smt_state_fetch(&self.state, key.as_ptr(), value.as_mut_ptr()) } {
            0 => Ok(value),
            err => Err(err as u32),
        }
    }

    pub fn normalize(&mut self) {
        unsafe {
            smt_state_normalize(&mut self.state);
        }
    }

    pub fn calculate_root(&self, proof: &[u8]) -> Result<[u8; 32], SMTErrorCode> {
        let mut root = [0u8; 32];
        match unsafe {
            smt_calculate_root(
                root.as_mut_ptr(),
                &self.state,
                proof.as_ptr(),
                proof.len() as u32,
            )
        } {
            0 => Ok(root),
            err => Err(err as u32),
        }
    }

    pub fn verify(&mut self, root: &[u8; 32], proof: &[u8]) -> Result<(), SMTErrorCode> {
        match unsafe {
            smt_verify(
                root.as_ptr(),
                &self.state,
                proof.as_ptr(),
                proof.len() as u32,
            )
        } {
            0 => Ok(()),
            err => Err(err as u32),
        }
    }

    pub fn is_empty(&self) -> bool {
        self.state.len == 0
    }
}
