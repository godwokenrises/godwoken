use exit_withdraw_state_validator::{
    settlement::{self as s, physical::*, records::*},
    transport,
};
use gw_types::U256;

#[test]
fn only_four_counter_kinds_are_mutable() {
    for kind in 0..s::RECORD_KINDS {
        assert_eq!(
            mutable(kind),
            [s::PAID, s::SOURCES, s::RESERVES, s::ASSETS].contains(&kind)
        );
    }
    assert!(!mutable(usize::MAX));
}

#[test]
fn physical_namespaces_have_no_aliases() {
    assert_ne!(account_key(7, 0), asset_state_key(7));
    assert_ne!(account_key(7, 0), account_key(7, 1));
    assert_ne!(asset_state_key(7), epoch_state_key(7));
    let mut bad = account_key(7, 0);
    bad[8] = 1;
    assert!(namespace(bad).is_err());
    bad = asset_state_key(7);
    bad[31] = 255;
    assert!(namespace(bad).is_err());
}

#[test]
fn pages_are_bounded_canonical_and_counters_require_an_immutable_slot() {
    let mut page = Page::new();
    for i in 0..PAGE_ITEMS {
        let source = Source {
            account: 7,
            asset: i as u32,
            baseline: U256::one(),
            ..Default::default()
        };
        let key = source_key([1; 32], &source);
        page.insert((s::SOURCE_PLAN, key), encode(&source));
        page.insert(
            (s::SOURCES, key),
            encode(&SourceBudget {
                paid: U256::one(),
                ..Default::default()
            }),
        );
    }
    let raw = pack_page(&page);
    assert!(raw.len() <= MAX_PAGE_BYTES);
    assert_eq!(unpack_page(&raw).unwrap(), page);
    let mut trailing = raw.clone();
    trailing.push(0);
    assert!(unpack_page(&trailing).is_err());
    let source = Source {
        account: 7,
        asset: 100,
        ..Default::default()
    };
    page.insert(
        (s::SOURCE_PLAN, source_key([1; 32], &source)),
        encode(&source),
    );
    assert!(unpack_page(&pack_page(&page)).is_err());
    let mut orphan = Page::new();
    orphan.insert((s::SOURCES, [2; 32]), encode(&SourceBudget::default()));
    assert!(unpack_page(&pack_page(&orphan)).is_err());
}

#[test]
fn framed_payload_checks_length_count_and_content() {
    let payload = vec![7; transport::CHUNK * 2 + 3];
    let (header, chunks) = transport::frame(&payload);
    let decoded = transport::unframe(&header, 3, |i| {
        chunks
            .get(i - 3)
            .cloned()
            .ok_or(exit_withdraw_state_validator::Error::Encoding)
    })
    .unwrap();
    assert_eq!(decoded, payload);
    let mut wrong = chunks.clone();
    wrong[0][0] ^= 1;
    assert!(transport::unframe(&header, 0, |i| Ok(wrong[i].clone())).is_err());
    let mut bad = header.clone();
    bad[12..16].copy_from_slice(&0u32.to_le_bytes());
    assert!(transport::unframe(&bad, 0, |_| unreachable!()).is_err());
    let (inline, chunks) = transport::frame(b"small");
    assert!(chunks.is_empty());
    assert_eq!(
        transport::unframe(&inline, 0, |_| unreachable!()).unwrap(),
        b"small"
    );
}
