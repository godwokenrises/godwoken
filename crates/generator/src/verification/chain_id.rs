use crate::error::LockAlgorithmError;

pub struct ChainIdVerifier {
    chain_id: u64,
}

impl ChainIdVerifier {
    pub fn new(chain_id: u64) -> Self {
        Self { chain_id }
    }

    /// check chain id
    pub fn verify(&self, chain_id: u64) -> Result<(), LockAlgorithmError> {
        if self.chain_id != chain_id {
            return Err(LockAlgorithmError::InvalidSignature(format!(
                "Wrong chain_id, expected: {} actual: {}",
                self.chain_id, chain_id
            )));
        }
        Ok(())
    }
}
