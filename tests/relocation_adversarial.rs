//! Independently constructed dummy ELF pairs; no bytes are target-executed.
use op1_screen_capture::relocation::verify_relocations;
fn w16(b: &mut [u8], p: usize, n: u16) {
    b[p..p + 2].copy_from_slice(&n.to_le_bytes());
}
fn w32(b: &mut [u8], p: usize, n: u32) {
    b[p..p + 4].copy_from_slice(&n.to_le_bytes());
}
fn header(b: &mut [u8], kind: u16, count: u16, names: u16, table: u32) {
    b[..7].copy_from_slice(b"\x7fELF\x01\x01\x01");
    for (p, n) in [
        (16, kind),
        (18, 106),
        (40, 52),
        (46, 40),
        (48, count),
        (50, names),
    ] {
        w16(b, p, n);
    }
    w32(b, 20, 1);
    w32(b, 32, table);
}
fn section(b: &mut [u8], p: usize, v: [u32; 10]) {
    for (i, n) in v.into_iter().enumerate() {
        w32(b, p + i * 4, n);
    }
}
fn sym(b: &mut [u8], p: usize, name: u32, val: u32, size: u32, info: u8) {
    w32(b, p, name);
    w32(b, p + 4, val);
    w32(b, p + 8, size);
    b[p + 12] = info;
    w16(b, p + 14, 1);
}
fn fixture(base: u32) -> (Vec<u8>, Vec<u8>) {
    let strings = b"\0_allocate_payload\0_release_payload\0_op1_adapter_bind\0";
    let sn = b"\0.text\0.rela.text\0.symtab\0.strtab\0.shstrtab\0";
    let mut src = vec![0; 840];
    header(&mut src, 1, 6, 5, 600);
    src[400..400 + strings.len()].copy_from_slice(strings);
    src[500..500 + sn.len()].copy_from_slice(sn);
    section(&mut src, 640, [1, 1, 6, 0, 64, 32, 0, 0, 4, 0]);
    section(&mut src, 680, [7, 4, 0, 0, 256, 60, 3, 1, 4, 12]);
    section(&mut src, 720, [18, 2, 0, 0, 128, 80, 4, 4, 4, 16]);
    section(
        &mut src,
        760,
        [26, 3, 0, 0, 400, strings.len() as u32, 0, 0, 1, 0],
    );
    section(
        &mut src,
        800,
        [34, 3, 0, 0, 500, sn.len() as u32, 0, 0, 1, 0],
    );
    sym(&mut src, 144, 0, 0, 0, 3);
    sym(&mut src, 160, 1, 2, 2, 2);
    sym(&mut src, 176, 19, 4, 2, 2);
    sym(&mut src, 192, 36, 0, 2, 0x12);
    // CALL at8 with reloc offset10; immediate instructions at12,16,20,24.
    src[73] = 0xe3;
    for (p, hi) in [(76, true), (80, true), (84, false), (88, false)] {
        src[p] = if hi { 0x40 } else { 0 };
        src[p + 1] = 0xe1;
    }
    for (i, (off, kind, si, add)) in [
        (10, 10, 4, 0),
        (14, 7, 1, 2),
        (18, 7, 1, 4),
        (22, 6, 1, 2),
        (26, 6, 1, 4),
    ]
    .into_iter()
    .enumerate()
    {
        let p = 256 + i * 12;
        w32(&mut src, p, off);
        w32(&mut src, p + 4, (si << 8) | kind);
        w32(&mut src, p + 8, add);
    }
    let mut dst = vec![0; 800];
    header(&mut dst, 2, 5, 4, 600);
    w32(&mut dst, 24, base);
    w32(&mut dst, 28, 64);
    w16(&mut dst, 42, 32);
    w16(&mut dst, 44, 1);
    for (p, n) in [
        (64, 1),
        (68, 128),
        (72, base),
        (76, base),
        (80, 32),
        (84, 32),
        (88, 5),
        (92, 4),
    ] {
        w32(&mut dst, p, n);
    }
    dst[128..160].copy_from_slice(&src[64..96]);
    dst[400..400 + strings.len()].copy_from_slice(strings);
    dst[500..500 + sn.len()].copy_from_slice(sn);
    section(&mut dst, 640, [1, 1, 6, base, 128, 32, 0, 0, 4, 0]);
    section(&mut dst, 680, [18, 2, 0, 0, 256, 80, 3, 4, 4, 16]);
    section(
        &mut dst,
        720,
        [26, 3, 0, 0, 400, strings.len() as u32, 0, 0, 1, 0],
    );
    section(
        &mut dst,
        760,
        [34, 3, 0, 0, 500, sn.len() as u32, 0, 0, 1, 0],
    );
    sym(&mut dst, 272, 0, base, 0, 3);
    sym(&mut dst, 288, 1, base + 2, 2, 2);
    sym(&mut dst, 304, 19, base + 4, 2, 2);
    sym(&mut dst, 320, 36, base, 2, 0x12);
    // Independent encoding: CALL target binder at0 gives -8-byte delta -> -4 halfwords.
    dst[136] = 0xff;
    w16(&mut dst, 138, 0xfffc);
    for (off, target, high) in [
        (14, base + 2, true),
        (18, base + 4, true),
        (22, base + 2, false),
        (26, base + 4, false),
    ] {
        w16(
            &mut dst,
            128 + off,
            if high {
                (target >> 16) as u16
            } else {
                target as u16
            },
        );
    }
    (src, dst)
}
#[test]
fn independent_profile_across_high_low_address_bits() {
    for base in [0x1000, 0xfffc, 0x12341000, 0xffff_0000] {
        let (s, d) = fixture(base);
        assert!(
            verify_relocations(&s, &d).is_ok(),
            "{base:x}: {:?}",
            verify_relocations(&s, &d)
        );
    }
}
#[test]
fn every_final_text_byte_corruption_rejects() {
    let (s, d) = fixture(0x12341000);
    for p in 128..160 {
        for bit in 0..8 {
            let mut m = d.clone();
            m[p] ^= 1 << bit;
            assert!(verify_relocations(&s, &m).is_err(), "{p}/{bit}");
        }
    }
}
#[test]
fn source_text_and_function_provenance_mismatch_rejects() {
    let (s, d) = fixture(0x12341000);
    for p in [64, 65, 66, 69, 73, 77, 81, 85, 89, 168, 184, 196] {
        let mut m = s.clone();
        m[p] ^= 1;
        assert!(verify_relocations(&m, &d).is_err(), "source {p}");
    }
    for p in [292, 308, 324] {
        let mut m = d.clone();
        m[p] ^= 2;
        assert!(verify_relocations(&s, &m).is_err(), "final sym {p}");
    }
}
#[test]
fn missing_duplicate_overlapping_wrong_relocation_rejects() {
    let (s, d) = fixture(0x12341000);
    for (p, n) in [
        (700, 48),
        (700, 72),
        (256, 0),
        (256, 1),
        (256, 33),
        (260, 0xffff_ff0a),
        (260, 4 * 256 + 9),
        (264, 1),
        (268, 10),
        (268, 12),
        (272, 256 + 6),
        (276, 4),
        (280, 14),
        (288, 2),
    ] {
        let mut m = s.clone();
        w32(&mut m, p, n);
        assert!(verify_relocations(&m, &d).is_err(), "{p}/{n}");
    }
}
#[test]
fn every_truncation_rejects_without_panicking() {
    let (s, d) = fixture(0x1000);
    for n in 0..s.len() {
        assert!(verify_relocations(&s[..n], &d).is_err(), "src {n}");
    }
    for n in 0..d.len() {
        assert!(verify_relocations(&s, &d[..n]).is_err(), "dst {n}");
    }
}
#[test]
fn mismatched_artificial_bases_reject() {
    let (s, mut d) = fixture(0x1000);
    let (_, other) = fixture(0x12341000);
    d[128..160].copy_from_slice(&other[128..160]);
    assert!(verify_relocations(&s, &d).is_err());
}

fn r16(b: &[u8], p: usize) -> usize {
    u16::from_le_bytes(b[p..p + 2].try_into().unwrap()) as usize
}
fn r32(b: &[u8], p: usize) -> usize {
    u32::from_le_bytes(b[p..p + 4].try_into().unwrap()) as usize
}
fn section_offset(b: &[u8], kind: usize) -> (usize, usize, usize) {
    let table = r32(b, 32);
    let count = r16(b, 48);
    let matches: Vec<_> = (1..count)
        .map(|i| table + i * 40)
        .filter(|p| r32(b, p + 4) == kind)
        .collect();
    assert_eq!(
        matches.len(),
        1,
        "expected unique fixture section kind {kind}"
    );
    let p = matches[0];
    (p, r32(b, p + 16), r32(b, p + 20))
}
#[test]
#[ignore = "explicit offline BF524 artifact integration; requires REL_INPUT and LINKED_INPUT"]
fn actual_artifact_semantics_and_corruptions() {
    let s = std::fs::read(std::env::var("REL_INPUT").expect("REL_INPUT")).unwrap();
    let d = std::fs::read(std::env::var("LINKED_INPUT").expect("LINKED_INPUT")).unwrap();
    let report = verify_relocations(&s, &d).expect("actual generated pair");
    assert_eq!(report.checked_relocations, 5);
    let (_, text, len) = section_offset(&d, 1);
    for p in text..text + len {
        let mut m = d.clone();
        m[p] ^= 1;
        assert!(verify_relocations(&s, &m).is_err(), "linked text byte {p}");
    }
    let (rh, relocs, rlen) = section_offset(&s, 4);
    assert_eq!(rlen, 60);
    for p in (relocs..relocs + rlen).step_by(12) {
        for at in [p, p + 4, p + 8] {
            let mut m = s.clone();
            m[at] ^= 1;
            assert!(
                verify_relocations(&m, &d).is_err(),
                "source relocation byte {at}"
            );
        }
    }
    let mut missing = s.clone();
    w32(&mut missing, rh + 20, 48);
    assert!(verify_relocations(&missing, &d).is_err());
    let mut duplicate = s.clone();
    let first = s[relocs..relocs + 12].to_vec();
    duplicate[relocs + 12..relocs + 24].copy_from_slice(&first);
    assert!(verify_relocations(&duplicate, &d).is_err());
    let (_, st, sl) = section_offset(&d, 2);
    for p in (st..st + sl).step_by(16).filter(|p| d[p + 12] & 15 == 2) {
        let mut m = d.clone();
        m[p + 4] ^= 2;
        assert!(verify_relocations(&s, &m).is_err(), "final function {p}");
    }
    let (_, source_symbols, symbol_bytes) = section_offset(&s, 2);
    for p in (source_symbols..source_symbols + symbol_bytes)
        .step_by(16)
        .filter(|p| s[p + 12] & 15 == 2)
    {
        let mut m = s.clone();
        m[p + 12] &= 0xf0;
        assert!(
            verify_relocations(&m, &d).is_err(),
            "downgraded source function {p}"
        );
        let mut m = s.clone();
        w16(&mut m, p + 14, 0);
        assert!(
            verify_relocations(&m, &d).is_err(),
            "undefined source function {p}"
        );
    }
    // Deliberately altered non-relocation source byte cannot masquerade as original code.
    let table = r32(&s, 32);
    let mut altered = s.clone();
    let source_text = r32(&s, table + 40 + 16);
    altered[source_text] ^= 1;
    assert!(verify_relocations(&altered, &d).is_err());
}

#[test]
fn unsupported_source_symbol_shapes_reject() {
    let (s, d) = fixture(0x1000);
    for p in [160usize, 176, 192] {
        for info in [0, 1, 0x10, 0x11, 0x22, 0xf2] {
            let mut m = s.clone();
            m[p + 12] = info;
            assert!(
                verify_relocations(&m, &d).is_err(),
                "symbol type {p}/{info}"
            );
        }
        for index in [0, 0xfff1, 0xfff2, 0xffff] {
            let mut m = s.clone();
            w16(&mut m, p + 14, index);
            assert!(
                verify_relocations(&m, &d).is_err(),
                "symbol section {p}/{index}"
            );
        }
    }
}
