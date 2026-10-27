//! Narrow fixed-link envelope validation, not a loader or relocation evaluator.
//!
//! Accepts only a non-writable, fully file-backed ELF32 Blackfin executable with
//! ordinary section/symbol tables. Absence of relocations is necessary, not proof
//! of their correct application. Linker/input provenance and target execution,
//! ownership, cache visibility, hook completeness and hardware safety are separate.

fn field16(b: &[u8], p: usize) -> Result<u16, String> {
    let s = b
        .get(p..p.checked_add(2).ok_or("field overflow")?)
        .ok_or("truncated field")?;
    Ok(u16::from_le_bytes([s[0], s[1]]))
}
fn field32(b: &[u8], p: usize) -> Result<u32, String> {
    let s = b
        .get(p..p.checked_add(4).ok_or("field overflow")?)
        .ok_or("truncated field")?;
    Ok(u32::from_le_bytes([s[0], s[1], s[2], s[3]]))
}
fn extent(start: u64, size: u64) -> Result<u64, String> {
    let end = start.checked_add(size).ok_or("extent overflow")?;
    if end > 1u64 << 32 {
        return Err("extent exceeds 32-bit address space".into());
    }
    Ok(end)
}
fn file_range(b: &[u8], start: u32, size: u32) -> Result<&[u8], String> {
    let end = u64::from(start) + u64::from(size);
    if end > b.len() as u64 {
        return Err("file extent out of bounds".into());
    }
    Ok(&b[start as usize..end as usize])
}
fn aligned(value: u32, align: u32) -> bool {
    align <= 1 || (align.is_power_of_two() && value.is_multiple_of(align))
}
fn cstr(table: &[u8], at: u32) -> Result<(), String> {
    if !table
        .get(at as usize..)
        .ok_or("string offset out of bounds")?
        .contains(&0)
    {
        return Err("unterminated string".into());
    }
    Ok(())
}
#[derive(Clone, Copy)]
struct Section {
    kind: u32,
    flags: u32,
    addr: u32,
    off: u32,
    size: u32,
    link: u32,
    info: u32,
    entsize: u32,
}
#[derive(Clone, Copy)]
struct Segment {
    addr: u32,
    off: u32,
    size: u32,
    flags: u32,
}

/// Validate a deliberately narrow, read-only fixed-linked module envelope.
///
/// `code_end` is exclusive. All load bytes must be accounted for by disjoint
/// allocated sections; no implicit segment padding/data is admitted. An ordinary
/// symbol table is mandatory. Caller must separately pin and trust exact inputs,
/// linker command/toolchain and produced artifact. This never authorizes loading.
pub fn inspect_linked_elf(
    b: &[u8],
    code_start: u64,
    code_end: u64,
    expected_entry: u32,
) -> Result<(), String> {
    if code_start >= code_end || code_end > 1u64 << 32 {
        return Err("invalid code reservation".into());
    }
    if b.len() < 52
        || b.get(..7) != Some(b"\x7fELF\x01\x01\x01")
        || b[7] != 0
        || b[8..16].iter().any(|v| *v != 0)
    {
        return Err("expected System V little-endian ELF32 version 1".into());
    }
    if field16(b, 16)? != 2
        || field16(b, 18)? != 106
        || field32(b, 20)? != 1
        || field32(b, 36)? != 0
    {
        return Err("expected ordinary Blackfin ET_EXEC without processor flags".into());
    }
    if field16(b, 40)? != 52 || field16(b, 42)? != 32 || field16(b, 46)? != 40 {
        return Err("unsupported header sizes".into());
    }
    let entry = field32(b, 24)?;
    if entry != expected_entry || entry % 2 != 0 {
        return Err("entry mismatch or misalignment".into());
    }
    let phoff = field32(b, 28)?;
    let shoff = field32(b, 32)?;
    let phnum = field16(b, 44)?;
    let shnum = field16(b, 48)?;
    let shstr = field16(b, 50)?;
    if phnum == 0
        || phnum == 0xffff
        || !(2..0xff00).contains(&shnum)
        || shstr == 0
        || shstr >= shnum
        || shstr >= 0xff00
        || phoff < 52
        || shoff < 52
    {
        return Err("missing or extended tables are unsupported".into());
    }
    file_range(b, phoff, u32::from(phnum) * 32)?;
    file_range(b, shoff, u32::from(shnum) * 40)?;
    let phend = u64::from(phoff) + u64::from(phnum) * 32;
    let shend = u64::from(shoff) + u64::from(shnum) * 40;
    if u64::from(phoff) < shend && u64::from(shoff) < phend {
        return Err("header tables overlap".into());
    }
    let mut sections = Vec::new();
    for i in 0..shnum {
        let p = shoff as usize + usize::from(i) * 40;
        if i == 0 && b[p..p + 40].iter().any(|v| *v != 0) {
            return Err("non-null section zero".into());
        }
        let s = Section {
            kind: field32(b, p + 4)?,
            flags: field32(b, p + 8)?,
            addr: field32(b, p + 12)?,
            off: field32(b, p + 16)?,
            size: field32(b, p + 20)?,
            link: field32(b, p + 24)?,
            info: field32(b, p + 28)?,
            entsize: field32(b, p + 36)?,
        };
        let align = field32(b, p + 32)?;
        if align > 1 && !align.is_power_of_two() {
            return Err("invalid section alignment".into());
        }
        if s.kind != 8 {
            file_range(b, s.off, s.size)?;
        }
        if (s.kind == 4 || s.kind == 9) && s.size != 0 {
            return Err("residual relocation records".into());
        }
        if matches!(s.kind, 6 | 11 | 17 | 18) || s.flags & (0x400 | 0x800) != 0 {
            return Err("dynamic, grouped, extended or compressed sections unsupported".into());
        }
        if s.flags & 2 != 0 {
            if s.kind != 1 || s.flags & !6 != 0 || !aligned(s.addr, align) || !aligned(s.off, align)
            {
                return Err("unsupported allocated section".into());
            }
            let end = extent(u64::from(s.addr), u64::from(s.size))?;
            if u64::from(s.addr) < code_start || end > code_end {
                return Err("allocated section outside reservation".into());
            }
        } else if s.flags & 4 != 0 {
            return Err("nonallocated executable section".into());
        }
        sections.push(s);
    }
    let names = sections[usize::from(shstr)];
    if names.kind != 3 || names.size == 0 {
        return Err("invalid section string table".into());
    }
    let names = file_range(b, names.off, names.size)?;
    if names[0] != 0 {
        return Err("invalid string table prefix".into());
    }
    for i in 0..shnum {
        cstr(names, field32(b, shoff as usize + usize::from(i) * 40)?)?;
    }
    let mut segments = Vec::new();
    for i in 0..phnum {
        let p = phoff as usize + usize::from(i) * 32;
        // This fixed-link subset has only LOAD headers, no interpreter/dynamic/TLS.
        if field32(b, p)? != 1 {
            return Err("non-LOAD program header unsupported".into());
        }
        let seg = Segment {
            off: field32(b, p + 4)?,
            addr: field32(b, p + 8)?,
            size: field32(b, p + 16)?,
            flags: field32(b, p + 24)?,
        };
        let align = field32(b, p + 28)?;
        if field32(b, p + 12)? != seg.addr
            || field32(b, p + 20)? != seg.size
            || seg.size == 0
            || !matches!(seg.flags, 4 | 5)
        {
            return Err("unsupported physical address, zero-fill or load permissions".into());
        }
        if align > 1 && (!align.is_power_of_two() || seg.addr % align != seg.off % align) {
            return Err("invalid segment alignment".into());
        }
        file_range(b, seg.off, seg.size)?;
        let end = extent(u64::from(seg.addr), u64::from(seg.size))?;
        if u64::from(seg.addr) < code_start || end > code_end {
            return Err("load outside reservation".into());
        }
        segments.push(seg);
    }
    let mut alloc: Vec<_> = sections
        .iter()
        .filter(|s| s.flags & 2 != 0 && s.size != 0)
        .collect();
    alloc.sort_by_key(|s| s.addr);
    for pair in alloc.windows(2) {
        if u64::from(pair[0].addr) + u64::from(pair[0].size) > u64::from(pair[1].addr) {
            return Err("allocated sections overlap".into());
        }
    }
    segments.sort_by_key(|s| s.addr);
    for pair in segments.windows(2) {
        if u64::from(pair[0].addr) + u64::from(pair[0].size) > u64::from(pair[1].addr) {
            return Err("LOAD segments overlap".into());
        }
    }
    for s in &alloc {
        let owners = segments
            .iter()
            .filter(|seg| {
                u64::from(s.addr) >= u64::from(seg.addr)
                    && u64::from(s.addr) + u64::from(s.size)
                        <= u64::from(seg.addr) + u64::from(seg.size)
                    && u64::from(s.off)
                        == u64::from(seg.off) + u64::from(s.addr) - u64::from(seg.addr)
                    && (s.flags & 4 == 0 || seg.flags & 1 != 0)
            })
            .count();
        if owners != 1 {
            return Err("section does not match one LOAD mapping".into());
        }
    }
    for seg in &segments {
        let mut cursor = u64::from(seg.addr);
        for s in alloc.iter().filter(|s| {
            s.addr >= seg.addr && u64::from(s.addr) < u64::from(seg.addr) + u64::from(seg.size)
        }) {
            if u64::from(s.addr) != cursor {
                return Err("unaccounted LOAD bytes".into());
            }
            cursor += u64::from(s.size);
        }
        if cursor != u64::from(seg.addr) + u64::from(seg.size) {
            return Err("unaccounted LOAD bytes".into());
        }
    }
    if !alloc.iter().any(|s| {
        s.flags & 4 != 0
            && entry >= s.addr
            && u64::from(entry) < u64::from(s.addr) + u64::from(s.size)
    }) {
        return Err("entry is not in executable section".into());
    }
    let mut symtabs = 0;
    for s in &sections {
        if s.kind != 2 {
            continue;
        }
        symtabs += 1;
        if s.entsize != 16
            || s.size < 16
            || s.size % 16 != 0
            || s.link as usize >= sections.len()
            || s.info > s.size / 16
        {
            return Err("invalid symbol table".into());
        }
        let strings = sections[s.link as usize];
        if strings.kind != 3 || strings.size == 0 {
            return Err("invalid symbol strings".into());
        }
        let strings = file_range(b, strings.off, strings.size)?;
        if strings[0] != 0 {
            return Err("invalid symbol string prefix".into());
        }
        let symbols = file_range(b, s.off, s.size)?;
        for (i, sym) in symbols.as_chunks::<16>().0.iter().enumerate() {
            if i == 0 {
                if sym.iter().any(|v| *v != 0) {
                    return Err("invalid null symbol".into());
                }
                continue;
            }
            cstr(strings, field32(sym, 0)?)?;
            if sym[12] & 15 > 4 || sym[12] >> 4 > 2 || sym[13] > 3 {
                return Err("unsupported symbol type, binding or visibility".into());
            }
            let index = field16(sym, 14)?;
            if index == 0 || index == 0xfff2 || index == 0xffff {
                return Err("undefined, common or extended symbol".into());
            }
            if index == 0xfff1 {
                continue;
            }
            if index >= 0xff00 || usize::from(index) >= sections.len() {
                return Err("symbol section out of bounds".into());
            }
            let target = sections[usize::from(index)];
            let value = u64::from(field32(sym, 4)?);
            let end = extent(value, u64::from(field32(sym, 8)?))?;
            let lo = if target.flags & 2 != 0 {
                u64::from(target.addr)
            } else {
                0
            };
            if value < lo || end > lo + u64::from(target.size) {
                return Err("symbol outside section".into());
            }
        }
    }
    if symtabs != 1 {
        return Err("exactly one ordinary symbol table required".into());
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    fn w16(b: &mut [u8], p: usize, v: u16) {
        b[p..p + 2].copy_from_slice(&v.to_le_bytes());
    }
    fn w32(b: &mut [u8], p: usize, v: u32) {
        b[p..p + 4].copy_from_slice(&v.to_le_bytes());
    }
    fn fixture() -> Vec<u8> {
        let mut b = vec![0; 512];
        b[..7].copy_from_slice(b"\x7fELF\x01\x01\x01");
        for (p, v) in [
            (16, 2),
            (18, 106),
            (40, 52),
            (42, 32),
            (44, 1),
            (46, 40),
            (48, 5),
            (50, 4),
        ] {
            w16(&mut b, p, v);
        }
        for (p, v) in [
            (20, 1),
            (24, 0x1000),
            (28, 52),
            (32, 256),
            (52, 1),
            (56, 128),
            (60, 0x1000),
            (64, 0x1000),
            (68, 4),
            (72, 4),
            (76, 5),
            (80, 4),
        ] {
            w32(&mut b, p, v);
        }
        // .text, symbol table, symbol strings, section names (all names empty).
        for (p, v) in [
            (300, 1),
            (304, 6),
            (308, 0x1000),
            (312, 128),
            (316, 4),
            (328, 4),
            (340, 2),
            (352, 144),
            (356, 32),
            (360, 3),
            (364, 1),
            (368, 4),
            (372, 16),
            (380, 3),
            (392, 180),
            (396, 2),
            (408, 1),
            (420, 3),
            (432, 184),
            (436, 1),
            (448, 1),
        ] {
            w32(&mut b, p, v);
        }
        w32(&mut b, 164, 0x1000);
        w32(&mut b, 168, 4);
        b[172] = 0x12;
        w16(&mut b, 174, 1);
        b
    }
    #[test]
    fn valid_envelope() {
        assert!(inspect_linked_elf(&fixture(), 0x1000, 0x1100, 0x1000).is_ok());
    }
    #[test]
    fn rejects_every_truncation() {
        let b = fixture();
        for n in 0..456 {
            assert!(
                inspect_linked_elf(&b[..n], 0x1000, 0x1100, 0x1000).is_err(),
                "length {n}"
            );
        }
    }
    #[test]
    fn rejects_malformed_headers() {
        for (p, v) in [
            (16, 1),
            (18, 3),
            (40, 51),
            (42, 31),
            (44, 0),
            (46, 39),
            (48, 0),
            (50, 0),
        ] {
            let mut b = fixture();
            w16(&mut b, p, v);
            assert!(
                inspect_linked_elf(&b, 0x1000, 0x1100, 0x1000).is_err(),
                "field {p}"
            );
        }
    }
    #[test]
    fn rejects_malformed_mapping() {
        for (p, v) in [
            (64, 0x2000),
            (72, 8),
            (76, 7),
            (80, 3),
            (60, 0xffff_fffe),
            (312, 129),
            (316, 8),
            (304, 7),
            (300, 8),
            (328, 3),
            (52, 2),
        ] {
            let mut b = fixture();
            w32(&mut b, p, v);
            assert!(
                inspect_linked_elf(&b, 0x1000, 0x1100, 0x1000).is_err(),
                "field {p}"
            );
        }
    }
    #[test]
    fn rejects_residual_relocations_and_missing_symbols() {
        for kind in [4, 9, 11, 1] {
            let mut b = fixture();
            w32(&mut b, 340, kind);
            assert!(inspect_linked_elf(&b, 0x1000, 0x1100, 0x1000).is_err());
        }
    }
    #[test]
    fn rejects_bad_symbols() {
        for index in [0, 5, 0xfff2, 0xffff] {
            let mut b = fixture();
            w16(&mut b, 174, index);
            assert!(inspect_linked_elf(&b, 0x1000, 0x1100, 0x1000).is_err());
        }
        for (p, v) in [(160, 3), (164, 0x1005), (168, 5), (360, 99), (372, 15)] {
            let mut b = fixture();
            w32(&mut b, p, v);
            assert!(inspect_linked_elf(&b, 0x1000, 0x1100, 0x1000).is_err());
        }
    }
    #[test]
    fn rejects_wrong_reservation_and_entry() {
        let b = fixture();
        for (lo, hi, e) in [
            (0x1001, 0x1100, 0x1000),
            (0x1000, 0x1003, 0x1000),
            (0, 1u64 << 33, 0x1000),
            (0x1000, 0x1100, 0x1002),
        ] {
            assert!(inspect_linked_elf(&b, lo, hi, e).is_err());
        }
    }
    #[test]
    fn rejects_reserved_numbers_and_nonordinary_symbols() {
        let mut b = fixture();
        w16(&mut b, 48, 0xff00);
        assert!(inspect_linked_elf(&b, 0x1000, 0x1100, 0x1000).is_err());
        for index in [0xff00, 0xfff0, 0xfffe] {
            let mut b = fixture();
            w16(&mut b, 174, index);
            assert!(inspect_linked_elf(&b, 0x1000, 0x1100, 0x1000).is_err());
        }
        for (p, value) in [(172, 0x16), (172, 0x1a), (172, 0xa2), (173, 4)] {
            let mut b = fixture();
            b[p] = value;
            assert!(inspect_linked_elf(&b, 0x1000, 0x1100, 0x1000).is_err());
        }
    }
    #[test]
    fn rejects_bad_table_extents_and_string_termination() {
        for p in [28, 32, 352, 392, 432] {
            let mut b = fixture();
            w32(&mut b, p, u32::MAX);
            assert!(inspect_linked_elf(&b, 0x1000, 0x1100, 0x1000).is_err());
        }
        let mut b = fixture();
        w32(&mut b, 160, 1);
        b[181] = b'x';
        assert!(inspect_linked_elf(&b, 0x1000, 0x1100, 0x1000).is_err());
    }
}
