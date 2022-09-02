use gw_common::{CKB_SUDT_SCRIPT_ARGS, H256};
use gw_types::{
    bytes::Bytes,
    packed::{CellOutput, Script, WithdrawalRequestExtra},
    prelude::{Builder, Entity, Pack, Unpack},
};
use std::collections::HashMap;

// Fit ckb-indexer output_capacity_range [inclusive, exclusive]
pub const MAX_CAPACITY: u64 = u64::MAX - 1;

#[derive(thiserror::Error, Debug)]
#[error("minimal capacity {0}")]
pub struct MinimalCapacityError(pub u64);

#[derive(Clone)]
pub struct UserWithdrawals {
    lock: Script,
    ckb_values: Vec<WithdrawalValue>,
    sudt_values: HashMap<H256, Vec<WithdrawalValue>>,
}

impl UserWithdrawals {
    pub fn new(lock: Script) -> Self {
        UserWithdrawals {
            lock,
            ckb_values: Default::default(),
            sudt_values: Default::default(),
        }
    }

    pub fn len(&self) -> usize {
        self.sudt_values.values().flatten().count() + self.ckb_values.len()
    }

    pub fn is_empty(&self) -> bool {
        0 == self.len()
    }

    pub fn push_extra(
        &mut self,
        (extra, type_): (&WithdrawalRequestExtra, Option<Script>),
    ) -> Result<(), MinimalCapacityError> {
        let value = WithdrawalValue::from_extra(extra, type_);

        self.push(value)
    }

    pub fn extend_from_extras<'a>(
        &'a mut self,
        extra_pairs: impl IntoIterator<Item = (&'a WithdrawalRequestExtra, Option<Script>)>,
    ) -> Result<(), MinimalCapacityError> {
        let vals = extra_pairs
            .into_iter()
            .map(|(extra, type_)| WithdrawalValue::from_extra(extra, type_));

        self.extend(vals)
    }

    pub fn into_outputs(self) -> Option<Vec<(CellOutput, Bytes)>> {
        if self.is_empty() {
            return None;
        }

        let ckb_occupied_capacity =
            WithdrawalValue::new_ckb(0).estimate_occupied_capacity(self.lock.clone());

        let mut outputs = Vec::with_capacity(self.len());
        let mut surplus_capacity = 0;

        for value in self.ckb_values {
            if value.capacity < ckb_occupied_capacity {
                surplus_capacity += value.capacity;
            } else {
                outputs.push(value.to_output_and_data(self.lock.clone()));
            }
        }

        for mut value in self.sudt_values.into_values().flatten() {
            if 0 != surplus_capacity {
                value.capacity += surplus_capacity;
                surplus_capacity = 0;
            }

            outputs.push(value.to_output_and_data(self.lock.clone()));
        }

        Some(outputs)
    }

    #[cfg(test)]
    fn get_sudt(&self, type_hash: &H256) -> Option<&Vec<WithdrawalValue>> {
        self.sudt_values.get(type_hash)
    }

    fn push(&mut self, value: WithdrawalValue) -> Result<(), MinimalCapacityError> {
        let required_capacity = value.estimate_occupied_capacity(self.lock.clone());
        if value.capacity < required_capacity {
            return Err(MinimalCapacityError(required_capacity));
        }

        let surplus_capacity = self.merge_sudt(value);
        if 0 != surplus_capacity {
            self.merge_ckb(surplus_capacity);
        }

        Ok(())
    }

    fn extend(
        &mut self,
        values: impl IntoIterator<Item = WithdrawalValue>,
    ) -> Result<(), MinimalCapacityError> {
        for val in values {
            self.push(val)?;
        }

        Ok(())
    }

    fn merge_sudt(&mut self, mut value: WithdrawalValue) -> u64 {
        let (amount, sudt_script_hash) = match value.sudt.as_ref() {
            Some((amount, type_script)) => (amount, type_script.hash().into()),
            None => return value.capacity,
        };

        let sudt_values_mut = self.sudt_values.entry(sudt_script_hash).or_default();
        let surplus_capacity = match sudt_values_mut.last_mut() {
            None => {
                let surplus_capacity = value.take_surplus_capacity(self.lock.clone());
                sudt_values_mut.push(value);

                surplus_capacity
            }
            Some(unfulfilled) if unfulfilled.checked_add_sudt(*amount).is_some() => {
                if let Some((balance, _)) = unfulfilled.sudt.as_mut() {
                    *balance += *amount;
                }

                value.capacity
            }
            Some(_fulfilled) => {
                let surplus_capacity = value.take_surplus_capacity(self.lock.clone());
                sudt_values_mut.push(value);

                surplus_capacity
            }
        };

        // Decending by sudt balance
        sudt_values_mut.sort_unstable_by(|a, b| (b.sudt_balance().cmp(&a.sudt_balance())));

        surplus_capacity
    }

    fn merge_ckb(&mut self, mut surplus_capacity: u64) {
        match self.ckb_values.last_mut() {
            None => {
                self.ckb_values
                    .push(WithdrawalValue::new_ckb(surplus_capacity));
            }
            Some(unfulfilled) if unfulfilled.checked_add_capacity(surplus_capacity).is_some() => {
                unfulfilled.capacity += surplus_capacity;
            }
            Some(fulfilled) => {
                let occupied_capacity = fulfilled.estimate_occupied_capacity(self.lock.clone());

                if surplus_capacity >= occupied_capacity {
                    self.ckb_values
                        .push(WithdrawalValue::new_ckb(surplus_capacity));
                } else {
                    // Borrow capacity from previous fulfilled one
                    let borrowed_capacity = occupied_capacity - surplus_capacity;

                    fulfilled.capacity -= borrowed_capacity;
                    surplus_capacity += borrowed_capacity;

                    self.ckb_values
                        .push(WithdrawalValue::new_ckb(surplus_capacity));
                }
            }
        }

        // Decending by capacity,
        self.ckb_values
            .sort_unstable_by(|a, b| (b.capacity.cmp(&a.capacity)))
    }
}

#[derive(Default, Clone)]
struct WithdrawalValue {
    capacity: u64,
    sudt: Option<(u128, Script)>,
}

impl WithdrawalValue {
    fn new_ckb(capacity: u64) -> Self {
        WithdrawalValue {
            capacity,
            sudt: None,
        }
    }

    fn from_extra(extra: &WithdrawalRequestExtra, type_: Option<Script>) -> Self {
        let req = extra.raw();

        let sudt_script_hash: [u8; 32] = req.sudt_script_hash().unpack();
        if CKB_SUDT_SCRIPT_ARGS != sudt_script_hash {
            debug_assert_eq!(Some(sudt_script_hash), type_.as_ref().map(|s| s.hash()));
        } else {
            debug_assert!(type_.is_none());
            debug_assert_eq!(req.amount().unpack(), 0);
        }

        WithdrawalValue {
            capacity: req.capacity().unpack(),
            sudt: type_.map(|s| (req.amount().unpack(), s)),
        }
    }

    fn sudt_balance(&self) -> Option<u128> {
        Some(self.sudt.as_ref()?.0)
    }

    #[cfg(test)]
    fn sudt_type(&self) -> Option<&Script> {
        Some(&self.sudt.as_ref()?.1)
    }

    fn checked_add_capacity(&self, capacity: u64) -> Option<u64> {
        debug_assert!(self.capacity <= MAX_CAPACITY);

        { self.capacity }
            .checked_add(1) // Ensure MAX_CAPACITY
            .and_then(|c| c.checked_add(capacity))
    }

    fn checked_add_sudt(&self, amount: u128) -> Option<u128> {
        let (balance, _) = self.sudt.as_ref()?;
        balance.checked_add(amount)
    }

    fn take_surplus_capacity(&mut self, lock: Script) -> u64 {
        let occupied_capacity = self.estimate_occupied_capacity(lock);
        debug_assert!(self.capacity >= occupied_capacity);

        let surplus_capacity = self.capacity - occupied_capacity;
        self.capacity = occupied_capacity;

        surplus_capacity
    }

    fn to_output_and_data(&self, lock: Script) -> (CellOutput, Bytes) {
        let output = CellOutput::new_builder()
            .capacity(self.capacity.pack())
            .type_(self.sudt.as_ref().map(|(_, script)| script.clone()).pack())
            .lock(lock)
            .build();

        let data = self
            .sudt_balance()
            .map(|b| b.pack().as_bytes())
            .unwrap_or_else(Bytes::default);

        (output, data)
    }

    fn estimate_occupied_capacity(&self, lock: Script) -> u64 {
        let (output, data) = self.to_output_and_data(lock);
        output.occupied_capacity(data.len()).expect("no overflow")
    }
}

#[cfg(test)]
mod tests {
    use ckb_types::bytes::Buf;
    use gw_types::packed::{RawWithdrawalRequest, WithdrawalRequest};

    use super::*;

    const CKB: u64 = 10u64.pow(8);

    #[test]
    fn test_withdrawal_value() {
        const CAPACITY: u64 = 500 * CKB;
        const AMOUNT: u128 = 1000;

        let sudt_type = Script::new_builder()
            .args([1u8; 32].to_vec().pack())
            .build();

        let owner_lock = Script::new_builder()
            .args([2u8; 32].to_vec().pack())
            .build();

        let extra = new_extra(
            CAPACITY,
            AMOUNT,
            Some(sudt_type.clone()),
            owner_lock.clone(),
        );

        let mut value = WithdrawalValue::from_extra(&extra, Some(sudt_type.clone()));

        assert_eq!(value.capacity, CAPACITY);
        assert_eq!(value.sudt_balance(), Some(AMOUNT));
        assert_eq!(value.sudt_type().map(|s| s.hash()), Some(sudt_type.hash()));

        assert!(value.checked_add_capacity(u64::MAX).is_none());
        assert!(value.checked_add_sudt(u128::MAX).is_none());

        let occupied_capacity = value.estimate_occupied_capacity(owner_lock.clone());
        assert_eq!(
            value.take_surplus_capacity(owner_lock),
            CAPACITY - occupied_capacity
        );
    }

    #[test]
    fn test_withdrawal_value_max_capacity() {
        let extra = new_extra(MAX_CAPACITY, 0, None, Script::default());
        let value = WithdrawalValue::from_extra(&extra, None);

        assert_eq!(value.capacity, MAX_CAPACITY);
        assert_eq!(value.sudt_balance(), None);
        assert_eq!(value.sudt_type(), None);

        assert!(value.checked_add_capacity(1).is_none());
        assert!(value.checked_add_sudt(1).is_none());
    }

    #[test]
    fn test_user_withdrawals() {
        const CAPACITY: u64 = 1000 * CKB;
        const AMOUNT: u128 = 1;

        let lock = Script::default();
        let sudt_type = Script::new_builder()
            .args([1u8; 32].to_vec().pack())
            .build();

        let ckb_extra = new_extra(CAPACITY, 0, None, lock.clone());
        let sudt_extra = new_extra(CAPACITY, AMOUNT, Some(sudt_type.clone()), lock.clone());
        let sudt_value = WithdrawalValue::from_extra(&sudt_extra, Some(sudt_type.clone()));
        let sudt_occupied_capacity = sudt_value.estimate_occupied_capacity(lock.clone());

        let mut withdrawals = UserWithdrawals::new(lock);
        assert!(withdrawals.push(WithdrawalValue::new_ckb(1)).is_err());

        withdrawals
            .extend_from_extras(vec![(&ckb_extra, None)])
            .unwrap();
        withdrawals.push(sudt_value).unwrap();
        assert_eq!(withdrawals.len(), 2);

        check_value(
            &withdrawals.ckb_values,
            0,
            CAPACITY * 2 - sudt_occupied_capacity,
            None,
        )
        .unwrap();

        check_value(
            withdrawals.get_sudt(&sudt_type.hash().into()).unwrap(),
            0,
            sudt_occupied_capacity,
            Some((AMOUNT, &sudt_type)),
        )
        .unwrap();
    }

    #[test]
    fn test_user_withdrawals_merge_sudt() {
        const CAPACITY: u64 = 1000 * CKB;
        const AMOUNT: u128 = 1;

        let lock = Script::default();
        let sudt_type = Script::new_builder()
            .args([1u8; 32].to_vec().pack())
            .build();

        let sudt_extra = new_extra(CAPACITY, AMOUNT, Some(sudt_type.clone()), lock.clone());
        let sudt_value = WithdrawalValue::from_extra(&sudt_extra, Some(sudt_type.clone()));
        let sudt_occupied_capacity = sudt_value.estimate_occupied_capacity(lock.clone());

        let mut withdrawals = UserWithdrawals::new(lock);
        withdrawals.push(sudt_value.clone()).unwrap();
        assert_eq!(withdrawals.len(), 2);

        check_value(
            &withdrawals.ckb_values,
            0,
            CAPACITY - sudt_occupied_capacity,
            None,
        )
        .unwrap();

        check_value(
            withdrawals.get_sudt(&sudt_type.hash().into()).unwrap(),
            0,
            sudt_occupied_capacity,
            Some((AMOUNT, &sudt_type)),
        )
        .unwrap();

        // Merge same sudt
        withdrawals.push(sudt_value.clone()).unwrap();
        assert_eq!(withdrawals.len(), 2);

        check_value(
            &withdrawals.ckb_values,
            0,
            CAPACITY * 2 - sudt_occupied_capacity,
            None,
        )
        .unwrap();

        check_value(
            withdrawals.get_sudt(&sudt_type.hash().into()).unwrap(),
            0,
            sudt_occupied_capacity,
            Some((AMOUNT * 2, &sudt_type)),
        )
        .unwrap();

        // Split sudt if balance overflow
        let mut sudt_value = sudt_value;
        sudt_value.sudt = Some((u128::MAX, sudt_type.clone()));

        withdrawals.push(sudt_value).unwrap();
        assert_eq!(withdrawals.len(), 3);

        check_value(
            withdrawals.get_sudt(&sudt_type.hash().into()).unwrap(),
            0,
            sudt_occupied_capacity,
            Some((u128::MAX, &sudt_type)),
        )
        .unwrap();

        check_value(
            withdrawals.get_sudt(&sudt_type.hash().into()).unwrap(),
            1,
            sudt_occupied_capacity,
            Some((AMOUNT * 2, &sudt_type)),
        )
        .unwrap();

        check_value(
            &withdrawals.ckb_values,
            0,
            CAPACITY * 3 - sudt_occupied_capacity * 2,
            None,
        )
        .unwrap();
    }

    #[test]
    fn test_user_withdrawals_merge_ckb() {
        const CAPACITY: u64 = 1000 * CKB;

        let lock = Script::default();
        let ckb_extra = new_extra(CAPACITY, 0, None, lock.clone());
        let ckb_value = WithdrawalValue::from_extra(&ckb_extra, None);

        let mut withdrawals = UserWithdrawals::new(lock.clone());
        withdrawals.push(ckb_value.clone()).unwrap();
        assert_eq!(withdrawals.len(), 1);
        check_value(&withdrawals.ckb_values, 0, CAPACITY, None).unwrap();

        // Merge ckb
        withdrawals.push(ckb_value.clone()).unwrap();
        assert_eq!(withdrawals.len(), 1);
        check_value(&withdrawals.ckb_values, 0, CAPACITY * 2, None).unwrap();

        // Split ckb if balance overflow
        let mut second_ckb_value = ckb_value.clone();
        second_ckb_value.capacity = MAX_CAPACITY;

        withdrawals.push(second_ckb_value).unwrap();
        assert_eq!(withdrawals.len(), 2);

        // NOTE: values are sorted by capacity in decending order
        check_value(&withdrawals.ckb_values, 0, MAX_CAPACITY, None).unwrap();
        check_value(&withdrawals.ckb_values, 1, CAPACITY * 2, None).unwrap();

        // Borrow ckb if occupied capacity is not enough
        // Raise all value to MAX_CAPACITY
        let last_value_capacity = withdrawals.ckb_values.last().unwrap().capacity;

        let mut third_ckb_value = ckb_value;
        let ckb_occupied_capacity = third_ckb_value.estimate_occupied_capacity(lock.clone());
        third_ckb_value.capacity = MAX_CAPACITY - last_value_capacity;

        // Use sudt value to create borrow case
        let sudt_type = Script::default();
        let sudt_extra = new_extra(CAPACITY, 1, Some(sudt_type.clone()), lock.clone());
        let sudt_value = WithdrawalValue::from_extra(&sudt_extra, Some(sudt_type));
        let sudt_occupied_capacity = sudt_value.estimate_occupied_capacity(lock);

        let mut borrower_value = sudt_value;
        borrower_value.capacity = sudt_occupied_capacity + ckb_occupied_capacity - 1;

        withdrawals
            .extend(vec![third_ckb_value, borrower_value])
            .unwrap();
        assert_eq!(withdrawals.len(), 4); // +1 sudt

        check_value(&withdrawals.ckb_values, 2, ckb_occupied_capacity, None).unwrap();
        check_value(&withdrawals.ckb_values, 1, MAX_CAPACITY - 1, None).unwrap();
    }

    #[test]
    fn test_user_withdrawals_into_outputs() {
        const CAPACITY: u64 = 1000 * CKB;
        const AMOUNT: u128 = 1;

        let lock = Script::default();

        let mut withdrawals = UserWithdrawals::new(lock);
        assert!(withdrawals.clone().into_outputs().is_none());

        let lock = Script::default();
        let sudt_type = Script::default();

        let sudt_extra = new_extra(CAPACITY, AMOUNT, Some(sudt_type.clone()), lock.clone());
        let mut sudt_value = WithdrawalValue::from_extra(&sudt_extra, Some(sudt_type.clone()));

        // Create non-zero `surplus_capacity`
        let sudt_occupied_capacity = sudt_value.estimate_occupied_capacity(lock.clone());
        sudt_value.capacity = sudt_occupied_capacity + 1;

        withdrawals.push(sudt_value.clone()).unwrap();
        assert_eq!(withdrawals.len(), 2);

        let outputs = withdrawals.clone().into_outputs().unwrap();
        assert_eq!(outputs.len(), 1);

        let (sudt_output, data) = outputs.first().unwrap();
        assert_eq!(sudt_output.capacity().unpack(), sudt_value.capacity);
        assert_eq!(
            sudt_output.type_().to_opt().map(|s| s.hash()),
            Some(sudt_type.hash())
        );
        assert_eq!(sudt_output.lock().hash(), lock.hash());

        let balance = data.clone().get_u128_le();
        assert_eq!(Some(balance), sudt_value.sudt_balance());

        let ckb_extra = new_extra(CAPACITY, 0, None, lock.clone());
        let ckb_value = WithdrawalValue::from_extra(&ckb_extra, None);

        withdrawals.push(ckb_value).unwrap();
        assert_eq!(withdrawals.len(), 2);

        let outputs = withdrawals.clone().into_outputs().unwrap();
        assert_eq!(outputs.len(), 2);

        let (sudt_output, _) = outputs.last().unwrap();
        assert_eq!(sudt_output.capacity().unpack(), sudt_occupied_capacity);

        let (ckb_output, data) = outputs.first().unwrap();
        assert_eq!(ckb_output.capacity().unpack(), CAPACITY + 1);
        assert!(data.is_empty());
        assert_eq!(ckb_output.type_().to_opt(), None,);
        assert_eq!(ckb_output.lock().hash(), lock.hash());
    }

    #[test]
    #[should_panic]
    fn test_invalid_withrawal_value_insufficient_capacity() {
        let extra = new_extra(61 * CKB, 1, Some(Script::default()), Script::default());
        let mut value = WithdrawalValue::from_extra(&extra, Some(Script::default()));

        value.take_surplus_capacity(Script::default());
    }

    #[test]
    #[should_panic]
    fn test_invalid_withdawal_value_sudt_type_hash_mismatch() {
        let raw_withdrawal = RawWithdrawalRequest::new_builder()
            .capacity((10000u64 * CKB).pack())
            .amount(1000u128.pack())
            .sudt_script_hash([5u8; 32].pack())
            .owner_lock_hash(Script::default().hash().pack())
            .build();

        let withdrawal_extra = WithdrawalRequestExtra::new_builder()
            .request(WithdrawalRequest::new_builder().raw(raw_withdrawal).build())
            .owner_lock(Script::default())
            .build();

        WithdrawalValue::from_extra(&withdrawal_extra, Some(Script::default()));
    }

    #[test]
    #[should_panic]
    fn test_invalid_withdawal_value_non_zero_sudt_amount() {
        let withdrawal_extra = new_extra(1000 * CKB, 1, None, Script::default());

        WithdrawalValue::from_extra(&withdrawal_extra, Some(Script::default()));
    }

    fn new_extra(
        capacity: u64,
        amount: u128,
        type_: Option<Script>,
        lock: Script,
    ) -> WithdrawalRequestExtra {
        let sudt_script_hash = type_.map(|s| s.hash()).unwrap_or([0u8; 32]);

        let raw_withdrawal = RawWithdrawalRequest::new_builder()
            .capacity(capacity.pack())
            .amount(amount.pack())
            .sudt_script_hash(sudt_script_hash.pack())
            .owner_lock_hash(lock.hash().pack())
            .build();

        WithdrawalRequestExtra::new_builder()
            .request(WithdrawalRequest::new_builder().raw(raw_withdrawal).build())
            .owner_lock(lock)
            .build()
    }

    fn check_value(
        values: &[WithdrawalValue],
        idx: usize,
        capacity: u64,
        sudt: Option<(u128, &Script)>,
    ) -> anyhow::Result<()> {
        use anyhow::{bail, Context};

        let value = values.get(idx).context("value not found")?;

        if value.capacity != capacity {
            bail!("capacity diff {}", value.capacity);
        }

        match (value.sudt.as_ref(), sudt) {
            (Some((balance, type_)), Some((amount, otype_))) => {
                if *balance != amount {
                    bail!("sudt balance diff {}", balance);
                }
                if type_.hash() != otype_.hash() {
                    bail!("sudt type_ diff {}", type_.hash().pack());
                }
            }
            (None, None) => (),
            _ => bail!("sudt diff"),
        }

        Ok(())
    }
}
