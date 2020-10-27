use crate::core::CallType;
/// extension methods
use crate::packed::{CallContext, RawL2Transaction};
use crate::prelude::*;

impl RawL2Transaction {
    pub fn to_call_context(&self) -> CallContext {
        // NOTICE users are only allowed to send HandleMessage CallType txs
        CallContext::new_builder()
            .args(self.args())
            .call_type(CallType::HandleMessage.into())
            .from_id(self.from_id())
            .to_id(self.to_id())
            .build()
    }
}
