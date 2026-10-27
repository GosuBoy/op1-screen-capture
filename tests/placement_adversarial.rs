//! Independent bounded arithmetic/contract checks; all addresses are synthetic.
use op1_screen_capture::placement::*;
use std::collections::BTreeMap;
fn region(start: u64, bytes: u64, alignment: u64) -> Region {
    Region {
        start,
        bytes,
        alignment,
    }
}
fn fixture() -> (Manifest, InputHashes) {
    let h = "a".repeat(64);
    (
        Manifest {
            canonical_sha256: h.clone(),
            module_sha256: h.clone(),
            linked_sha256: h.clone(),
            heap: region(0x1000, 0x10000, 8),
            usable_start: 0x1040,
            cut: 0x10000,
            retained_stock_budget: 0x1000,
            code: region(0x10010, 0x400, 4),
            guard_before: region(0x10000, 16, 4),
            guard_after: region(0x10410, 16, 4),
            final_image_bytes: 0x400,
            entry: 0x10010,
            state_bytes: 92,
            bridge_bytes: 8,
            staging_bytes: 4096,
            runtime_alignment: 4,
            allocation_overhead_budget: 128,
            pre_registration: true,
            final_hook_inventory: true,
            runtime_contract: true,
            evidence: BTreeMap::new(),
        },
        InputHashes {
            canonical: h.clone(),
            module: h.clone(),
            linked: h,
        },
    )
}
#[test]
fn region_result_matches_independent_wide_oracle() {
    let points = [
        0,
        1,
        2,
        3,
        4,
        7,
        8,
        15,
        16,
        ADDRESS_END - 16,
        ADDRESS_END - 1,
        ADDRESS_END,
        ADDRESS_END + 1,
        u64::MAX,
    ];
    for start in points {
        for bytes in points {
            for align in [0, 1, 2, 3, 4, 8, 16, ADDRESS_END, ADDRESS_END + 1] {
                let expected = bytes > 0
                    && align.is_power_of_two()
                    && align <= ADDRESS_END
                    && start < ADDRESS_END
                    && start % align.max(1) == 0
                    && u128::from(start) + u128::from(bytes) <= u128::from(ADDRESS_END);
                assert_eq!(
                    region(start, bytes, align).end().is_ok(),
                    expected,
                    "{start}/{bytes}/{align}"
                );
            }
        }
    }
}
#[test]
fn exact_runtime_capacity_boundary_is_checked() {
    let (mut m, h) = fixture();
    let runtime = validate(&m, &h).unwrap().runtime_budget;
    m.retained_stock_budget = m.cut - m.usable_start - runtime;
    assert!(validate(&m, &h).is_ok());
    m.retained_stock_budget += 1;
    assert!(validate(&m, &h).is_err());
}
#[test]
fn every_identity_is_bound_and_malformed_hashes_reject() {
    for field in 0..3 {
        for bad in [
            "".to_owned(),
            "A".repeat(64),
            "g".repeat(64),
            "b".repeat(64),
        ] {
            let (m, mut actual) = fixture();
            match field {
                0 => actual.canonical = bad,
                1 => actual.module = bad,
                _ => actual.linked = bad,
            };
            assert!(validate(&m, &actual).is_err());
        }
    }
}
#[test]
fn all_guard_offsets_except_exact_adjacency_reject() {
    for offset in -16i64..=16 {
        let (mut m, h) = fixture();
        m.guard_after.start = (m.guard_after.start as i64 + offset) as u64;
        assert_eq!(validate(&m, &h).is_ok(), offset == 0);
    }
}
#[test]
fn evidence_strings_do_not_upgrade_runtime_or_identity() {
    let (mut m, h) = fixture();
    for key in EVIDENCE_KEYS {
        m.evidence.insert(key.into(), "unverified reference".into());
    }
    assert!(validate(&m, &h).unwrap().missing_evidence.is_empty());
    m.runtime_contract = false;
    assert!(validate(&m, &h).is_err());
    m.runtime_contract = true;
    m.pre_registration = false;
    assert!(validate(&m, &h).is_err());
}
#[test]
fn runtime_slack_and_overhead_cannot_wrap() {
    for overhead in [u64::MAX, u64::MAX - 100, 1u64 << 63] {
        let (mut m, h) = fixture();
        m.allocation_overhead_budget = overhead;
        assert!(validate(&m, &h).is_err());
    }
    for alignment in [0, 1, 2, 3, 5, ADDRESS_END, ADDRESS_END * 2] {
        let (mut m, h) = fixture();
        m.runtime_alignment = alignment;
        assert!(validate(&m, &h).is_err());
    }
}
#[test]
fn runtime_and_code_boundary_inputs_never_panic() {
    for value in [0, 1, 3, 4, 8, ADDRESS_END - 1, ADDRESS_END, u64::MAX] {
        for field in 0..11 {
            let (mut m, h) = fixture();
            match field {
                0 => m.heap.start = value,
                1 => m.heap.bytes = value,
                2 => m.usable_start = value,
                3 => m.cut = value,
                4 => m.code.bytes = value,
                5 => m.code.start = value,
                6 => m.guard_before.bytes = value,
                7 => m.guard_after.bytes = value,
                8 => m.staging_bytes = value,
                9 => m.final_image_bytes = value,
                _ => m.retained_stock_budget = value,
            };
            let _ = validate(&m, &h);
        }
    }
}
