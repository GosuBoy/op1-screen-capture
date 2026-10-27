use op1_screen_capture::placement::*;
use std::collections::BTreeMap;
fn r(start: u64, bytes: u64, alignment: u64) -> Region {
    Region {
        start,
        bytes,
        alignment,
    }
}
fn sample() -> (Manifest, InputHashes) {
    let hash = "a".repeat(64);
    (
        Manifest {
            canonical_sha256: hash.clone(),
            module_sha256: hash.clone(),
            linked_sha256: hash.clone(),
            heap: r(0x1000, 0x10000, 8),
            usable_start: 0x1040,
            cut: 0x10000,
            retained_stock_budget: 0x1000,
            code: r(0x10010, 0x400, 4),
            guard_before: r(0x10000, 16, 4),
            guard_after: r(0x10410, 16, 4),
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
            canonical: hash.clone(),
            module: hash.clone(),
            linked: hash,
        },
    )
}
#[test]
fn synthetic_geometry_passes_but_evidence_missing() {
    let (m, h) = sample();
    let s = validate(&m, &h).unwrap();
    assert_eq!(s.missing_evidence.len(), 6);
    assert_eq!(s.runtime_budget, 4333);
}
#[test]
fn identity_is_not_just_syntax() {
    let (m, mut h) = sample();
    h.linked = "b".repeat(64);
    assert!(validate(&m, &h).is_err());
}
#[test]
fn rejects_each_missing_contract() {
    for field in 0..3 {
        let (mut m, h) = sample();
        match field {
            0 => m.pre_registration = false,
            1 => m.final_hook_inventory = false,
            _ => m.runtime_contract = false,
        };
        assert!(validate(&m, &h).is_err());
    }
}
#[test]
fn exact_address_end_allowed_overflow_rejected() {
    assert_eq!(r(0xfffffff0, 16, 4).end(), Ok(ADDRESS_END));
    assert!(r(0xfffffff0, 17, 4).end().is_err());
    assert!(r(u64::MAX, 2, 1).end().is_err());
}
#[test]
fn bad_alignments() {
    for alignment in [0, 3, 6, ADDRESS_END + 1] {
        assert!(r(0, 4, alignment).end().is_err());
    }
    assert!(r(3, 4, 4).end().is_err());
    assert!(r(0, 0, 1).end().is_err());
}
#[test]
fn overlap_and_nonadjacent_guards_rejected() {
    let (mut m, h) = sample();
    m.guard_before.bytes += 1;
    assert!(validate(&m, &h).is_err());
    let (mut m, h) = sample();
    m.guard_after.start += 4;
    assert!(validate(&m, &h).is_err());
}
#[test]
fn retained_capacity_accounts_runtime_and_slack() {
    let (mut m, h) = sample();
    m.retained_stock_budget = m.cut - m.usable_start;
    assert!(validate(&m, &h).is_err());
    m.retained_stock_budget = u64::MAX;
    assert!(validate(&m, &h).is_err());
}
#[test]
fn actual_abi_sizes_required() {
    for (state, bridge) in [(91, 8), (92, 7), (93, 8)] {
        let (mut m, h) = sample();
        m.state_bytes = state;
        m.bridge_bytes = bridge;
        assert!(validate(&m, &h).is_err());
    }
}
#[test]
fn invalid_cut_and_usable_start() {
    for cut in [0, 0x1040, 0x11000, 0x10001] {
        let (mut m, h) = sample();
        m.cut = cut;
        assert!(validate(&m, &h).is_err());
    }
}
#[test]
fn final_image_and_entry_are_bounded() {
    for size in [0, 0x401, u64::MAX] {
        let (mut m, h) = sample();
        m.final_image_bytes = size;
        assert!(validate(&m, &h).is_err());
    }
    for entry in [0, 0x10410, 0x10011] {
        let (mut m, h) = sample();
        m.entry = entry;
        assert!(validate(&m, &h).is_err());
    }
}
#[test]
fn runtime_overflow_and_zero_staging_fail() {
    for size in [0, u64::MAX] {
        let (mut m, h) = sample();
        m.staging_bytes = size;
        assert!(validate(&m, &h).is_err());
    }
}
#[test]
fn evidence_references_do_not_affect_arithmetic() {
    let (mut m, h) = sample();
    for key in EVIDENCE_KEYS {
        m.evidence
            .insert(key.into(), "review:synthetic-only".into());
    }
    assert!(validate(&m, &h).unwrap().missing_evidence.is_empty());
    m.code.start = 0;
    assert!(validate(&m, &h).is_err());
}
#[test]
fn parser_rejects_unknown_duplicate_and_missing() {
    assert!(parse("version=1\nversion=1").is_err());
    assert!(parse("version=2").is_err());
    assert!(parse("version=1").is_err());
    assert!(parse("garbage").is_err());
}
#[test]
fn example_parses_and_unknown_keys_fail() {
    let text = include_str!("../examples/placement-synthetic.manifest");
    let m = parse(text).unwrap();
    let (_, h) = sample();
    assert_eq!(validate(&m, &h).unwrap().missing_evidence.len(), 6);
    assert!(parse(&format!("{text}\nunknown=1")).is_err());
    assert!(parse(&format!("{text}\nentry=1")).is_err());
}
#[test]
fn absent_overhead_budget_fails() {
    let (mut m, h) = sample();
    m.allocation_overhead_budget = 0;
    assert!(validate(&m, &h).is_err());
}
