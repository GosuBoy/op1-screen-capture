//! Independent malformed ELF probes. Dummy instruction bytes are never executed.
use op1_screen_capture::placement_elf::inspect_linked_elf;
fn w16(b: &mut [u8], p: usize, n: u16) {
    b[p..p + 2].copy_from_slice(&n.to_le_bytes());
}
fn w32(b: &mut [u8], p: usize, n: u32) {
    b[p..p + 4].copy_from_slice(&n.to_le_bytes());
}
fn elf() -> Vec<u8> {
    let mut b = vec![0; 600];
    b[..7].copy_from_slice(b"\x7fELF\x01\x01\x01");
    for (p, n) in [
        (16, 2),
        (18, 106),
        (40, 52),
        (42, 32),
        (44, 1),
        (46, 40),
        (48, 5),
        (50, 4),
    ] {
        w16(&mut b, p, n);
    }
    for (p, n) in [
        (20, 1),
        (24, 0x8000),
        (28, 64),
        (32, 400),
        (64, 1),
        (68, 128),
        (72, 0x8000),
        (76, 0x8000),
        (80, 8),
        (84, 8),
        (88, 5),
        (92, 4),
    ] {
        w32(&mut b, p, n);
    }
    // section 1: text; 2: symbols; 3: symbol strings; 4: section strings
    for (p, n) in [
        (444, 1),
        (448, 6),
        (452, 0x8000),
        (456, 128),
        (460, 8),
        (472, 4),
        (484, 2),
        (496, 304),
        (500, 32),
        (504, 3),
        (508, 1),
        (512, 4),
        (516, 16),
        (524, 3),
        (536, 256),
        (540, 1),
        (552, 1),
        (564, 3),
        (576, 260),
        (580, 1),
        (592, 1),
    ] {
        w32(&mut b, p, n);
    }
    w32(&mut b, 324, 0x8000);
    w32(&mut b, 328, 8);
    b[332] = 0x12;
    w16(&mut b, 334, 1);
    b
}
fn ok(b: &[u8]) -> bool {
    inspect_linked_elf(b, 0x8000, 0x8010, 0x8000).is_ok()
}
#[test]
fn independent_fixture_and_all_truncations() {
    let b = elf();
    assert!(ok(&b));
    for n in 0..600 {
        assert!(!ok(&b[..n]), "truncated {n}");
    }
}
#[test]
fn omitted_or_unaccounted_load_bytes_reject() {
    for (p, n) in [
        (80, 12),
        (84, 12),
        (460, 4),
        (448, 0),
        (456, 132),
        (68, 132),
    ] {
        let mut b = elf();
        w32(&mut b, p, n);
        assert!(!ok(&b), "{p}/{n}");
    }
}
#[test]
fn memory_image_cannot_differ_from_file_or_address() {
    for (p, n) in [
        (76, 0x9000),
        (84, 16),
        (88, 7),
        (444, 8),
        (64, 3),
        (452, 0xffff_fffc),
    ] {
        let mut b = elf();
        w32(&mut b, p, n);
        assert!(!ok(&b), "{p}/{n}");
    }
}
#[test]
fn entry_and_symbol_cannot_escape_loaded_code() {
    for (p, n) in [(24, 0x8008), (324, 0x7ffe), (324, 0x8008), (328, 9)] {
        let mut b = elf();
        w32(&mut b, p, n);
        assert!(!ok(&b), "{p}/{n}");
    }
}
#[test]
fn deterministic_byte_mutations_never_panic() {
    let b = elf();
    for p in 0..b.len() {
        for n in [0, 1, 0x7f, 0xff] {
            let mut m = b.clone();
            m[p] = n;
            let _ = ok(&m);
        }
    }
}
#[test]
fn envelope_acceptance_is_not_instruction_semantics() {
    let mut b = elf();
    assert!(ok(&b));
    b[128..136].fill(0xff);
    assert!(
        ok(&b),
        "checker must be described as envelope-only, not instruction proof"
    );
}
