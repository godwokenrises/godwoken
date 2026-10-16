//! Frozen account pages. Mutable counters always occupy their immutable record's page.
use anyhow::{ensure, Context, Result};
use exit_withdraw_state_validator::{
    self as c,
    settlement::{self as s, physical, records::*},
};
use std::collections::BTreeMap;

pub type Records = BTreeMap<c::Hash, Vec<u8>>;

pub struct Layout {
    pub records: Records,
    locations: BTreeMap<(usize, c::Hash), c::Hash>,
}

#[cfg(test)]
mod tests {
    use super::*;
    use gw_types::U256;

    #[test]
    fn large_accounts_keep_static_slots_and_counter_pages_stable() {
        let deployment = [9; 32];
        let source = Source {
            account: 25,
            asset: 1,
            baseline: U256::from(80),
            ..Default::default()
        };
        let source_key = source_key(deployment, &source);
        let mut maps = std::array::from_fn(|_| Records::new());
        maps[0].insert(source_key, encode(&source));
        for i in 0u32..80 {
            let lot = Lot {
                id: gw_hash::blake2b::hash(&i.to_le_bytes()),
                source: source_key,
                kind: INITIAL,
                amount: U256::one(),
                ..Default::default()
            };
            let lot_key = lot_key(deployment, &lot);
            maps[1].insert(lot_key, encode(&lot));
            let claim = Claim {
                source: source_key,
                lot: lot_key,
                cap: U256::one(),
                ..Default::default()
            };
            maps[2].insert(claim_key(deployment, &claim), encode(&claim));
        }
        let layout = Layout::new(&maps).unwrap();
        assert_eq!(layout.records.len(), 11);
        let mut state = layout.records.clone();
        for (kind, entries) in [(s::RESERVES, &maps[1]), (s::PAID, &maps[2])] {
            for key in entries.keys() {
                let value = if kind == s::PAID {
                    encode(&Paid {
                        amount: U256::one(),
                        ..Default::default()
                    })
                } else {
                    encode(&Reserve::default())
                };
                let (page, raw) = layout.update_page(&state, kind, *key, value).unwrap();
                assert_eq!(
                    page,
                    layout
                        .location(
                            if kind == s::PAID {
                                s::CLAIM_PLAN
                            } else {
                                s::LOT_PLAN
                            },
                            *key
                        )
                        .unwrap()
                );
                state.insert(page, raw);
            }
        }
        assert_eq!(
            state.keys().collect::<Vec<_>>(),
            layout.records.keys().collect::<Vec<_>>()
        );
        for (key, before) in &layout.records {
            let before = physical::unpack_page(before).unwrap();
            let after = physical::unpack_page(&state[key]).unwrap();
            for (slot, value) in before {
                assert_eq!(after[&slot], value);
            }
        }
        assert!(layout
            .update_page(&state, s::SOURCE_PLAN, source_key, encode(&source))
            .is_err());
        assert!(layout
            .update_page(&state, s::PAID, [255; 32], encode(&Paid::default()))
            .is_err());
    }
}

fn decode_record<T: Field>(raw: &[u8]) -> Result<T> {
    decode(raw).map_err(|e| anyhow::anyhow!("Account layout: {e:?}"))
}

impl Layout {
    pub fn new(maps: &[Records; 3]) -> Result<Self> {
        let mut accounts = BTreeMap::<u32, BTreeMap<(usize, c::Hash), Vec<u8>>>::new();
        for (kind, records) in [s::SOURCE_PLAN, s::LOT_PLAN, s::CLAIM_PLAN]
            .into_iter()
            .zip(maps)
        {
            for (key, raw) in records {
                let account = if kind == s::SOURCE_PLAN {
                    decode_record::<Source>(raw)?.account
                } else {
                    let source = if kind == s::LOT_PLAN {
                        decode_record::<Lot>(raw)?.source
                    } else {
                        decode_record::<Claim>(raw)?.source
                    };
                    decode_record::<Source>(maps[0].get(&source).context("Missing page source")?)?
                        .account
                };
                accounts
                    .entry(account)
                    .or_default()
                    .insert((kind, *key), raw.clone());
            }
        }
        let mut records = BTreeMap::new();
        let mut locations = BTreeMap::new();
        for (account, entries) in accounts {
            let entries: Vec<_> = entries.into_iter().collect();
            for (index, chunk) in entries.chunks(physical::PAGE_ITEMS).enumerate() {
                let key = physical::account_key(account, u32::try_from(index)?);
                let mut page = physical::Page::new();
                for ((kind, logical), value) in chunk {
                    locations.insert((*kind, *logical), key);
                    page.insert((*kind, *logical), value.clone());
                }
                let raw = physical::pack_page(&page);
                physical::unpack_page(&raw).map_err(|e| anyhow::anyhow!("Account page: {e:?}"))?;
                records.insert(key, raw);
            }
        }
        Ok(Self { records, locations })
    }

    pub fn location(&self, kind: usize, key: c::Hash) -> Result<c::Hash> {
        let parent =
            physical::static_kind(kind).map_err(|e| anyhow::anyhow!("State kind: {e:?}"))?;
        self.locations
            .get(&(parent, key))
            .copied()
            .context("Missing immutable state slot")
    }

    pub fn update_page(
        &self,
        records: &Records,
        kind: usize,
        key: c::Hash,
        value: Vec<u8>,
    ) -> Result<(c::Hash, Vec<u8>)> {
        ensure!(
            physical::mutable(kind) && kind != s::ASSETS,
            "Immutable page write"
        );
        let location = self.location(kind, key)?;
        let mut page =
            physical::unpack_page(records.get(&location).context("Missing account page")?)
                .map_err(|e| anyhow::anyhow!("Account page: {e:?}"))?;
        page.insert((kind, key), value);
        let raw = physical::pack_page(&page);
        physical::unpack_page(&raw).map_err(|e| anyhow::anyhow!("Updated account page: {e:?}"))?;
        Ok((location, raw))
    }
}
