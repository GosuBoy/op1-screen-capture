//! Bounded semantic verification of the current adapter's five relocations.
//! Not a general linker: one unchanged .text, one PCREL24 and two HU/LO pairs.
use std::collections::{BTreeMap, BTreeSet};

fn u16at(b: &[u8], p: usize) -> Result<u16, String> {
    let x = b
        .get(p..p.checked_add(2).ok_or("offset overflow")?)
        .ok_or("truncated u16")?;
    Ok(u16::from_le_bytes([x[0], x[1]]))
}
fn u32at(b: &[u8], p: usize) -> Result<u32, String> {
    let x = b
        .get(p..p.checked_add(4).ok_or("offset overflow")?)
        .ok_or("truncated u32")?;
    Ok(u32::from_le_bytes([x[0], x[1], x[2], x[3]]))
}
fn slice(b: &[u8], off: u32, len: u32) -> Result<&[u8], String> {
    let end = u64::from(off) + u64::from(len);
    if end > b.len() as u64 {
        return Err("file extent overflow".into());
    }
    Ok(&b[off as usize..end as usize])
}
fn name(b: &[u8], at: u32) -> Result<String, String> {
    let b = b.get(at as usize..).ok_or("invalid string offset")?;
    let n = b
        .iter()
        .position(|x| *x == 0)
        .ok_or("unterminated string")?;
    Ok(std::str::from_utf8(&b[..n])
        .map_err(|_| "non UTF8 symbol")?
        .to_owned())
}
#[derive(Clone)]
struct Section {
    name: String,
    kind: u32,
    flags: u32,
    addr: u32,
    off: u32,
    size: u32,
    link: u32,
    info: u32,
    align: u32,
    entsize: u32,
}
#[derive(Clone)]
struct Symbol {
    name: String,
    value: u32,
    size: u32,
    info: u8,
    other: u8,
    section: u16,
}
struct Elf<'a> {
    b: &'a [u8],
    sections: Vec<Section>,
}
impl<'a> Elf<'a> {
    fn parse(b: &'a [u8], kind: u16) -> Result<Self, String> {
        if b.len() < 52
            || b.get(..16) != Some(&b"\x7fELF\x01\x01\x01\0\0\0\0\0\0\0\0\0"[..])
            || u16at(b, 16)? != kind
            || u16at(b, 18)? != 106
            || u32at(b, 20)? != 1
            || u32at(b, 36)? != 0
            || u16at(b, 40)? != 52
            || u16at(b, 46)? != 40
        {
            return Err("unsupported Blackfin ELF envelope".into());
        }
        if kind == 1 && (u32at(b, 24)? != 0 || u32at(b, 28)? != 0 || u16at(b, 44)? != 0) {
            return Err("REL has entry/program headers".into());
        }
        let off = u32at(b, 32)?;
        let count = u16at(b, 48)?;
        let names = u16at(b, 50)?;
        if off < 52 || !(2..0xff00).contains(&count) || names == 0 || names >= count {
            return Err("unsupported section numbering".into());
        }
        let table = slice(b, off, u32::from(count) * 40)?;
        if table[..40].iter().any(|x| *x != 0) {
            return Err("invalid null section".into());
        }
        let np = usize::from(names) * 40;
        if u32at(table, np + 4)? != 3 {
            return Err("invalid section name table".into());
        }
        let strings = slice(b, u32at(table, np + 16)?, u32at(table, np + 20)?)?;
        let mut sections = Vec::new();
        for i in 0..count {
            let p = usize::from(i) * 40;
            let s = Section {
                name: name(strings, u32at(table, p)?)?,
                kind: u32at(table, p + 4)?,
                flags: u32at(table, p + 8)?,
                addr: u32at(table, p + 12)?,
                off: u32at(table, p + 16)?,
                size: u32at(table, p + 20)?,
                link: u32at(table, p + 24)?,
                info: u32at(table, p + 28)?,
                align: u32at(table, p + 32)?,
                entsize: u32at(table, p + 36)?,
            };
            if s.kind != 8 {
                slice(b, s.off, s.size)?;
            }
            if s.align > 1 && !s.align.is_power_of_two() {
                return Err("invalid section alignment".into());
            }
            if kind == 1 && s.addr != 0 {
                return Err("REL section has address".into());
            }
            sections.push(s);
        }
        Ok(Self { b, sections })
    }
    fn section(&self, n: &str) -> Result<(usize, &Section), String> {
        let v: Vec<_> = self
            .sections
            .iter()
            .enumerate()
            .filter(|(_, s)| s.name == n)
            .collect();
        if v.len() != 1 {
            return Err(format!("expected unique {n}"));
        }
        Ok(v[0])
    }
    fn data(&self, s: &Section) -> Result<&'a [u8], String> {
        slice(self.b, s.off, s.size)
    }
    fn symbols(&self) -> Result<(usize, Vec<Symbol>), String> {
        let (idx, s) = self.section(".symtab")?;
        if s.kind != 2 || s.entsize != 16 || s.size % 16 != 0 || s.size == 0 {
            return Err("invalid symtab".into());
        }
        let t = self.sections.get(s.link as usize).ok_or("symtab link")?;
        if t.kind != 3 {
            return Err("symtab string link".into());
        }
        let strings = self.data(t)?;
        let bytes = self.data(s)?;
        let mut out = Vec::new();
        if bytes[..16].iter().any(|x| *x != 0) {
            return Err("invalid null symbol".into());
        }
        for p in (0..bytes.len()).step_by(16) {
            out.push(Symbol {
                name: name(strings, u32at(bytes, p)?)?,
                value: u32at(bytes, p + 4)?,
                size: u32at(bytes, p + 8)?,
                info: bytes[p + 12],
                other: bytes[p + 13],
                section: u16at(bytes, p + 14)?,
            });
        }
        Ok((idx, out))
    }
}
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RelocationReport {
    pub checked_relocations: usize,
    pub code_base: u32,
    pub text_bytes: usize,
}

/// Verify the current three-function bridge relocation shape and reconstruct all
/// final text bytes from the supplied original object. Caller must authenticate
/// exact object/link identities separately; neither file authenticates itself.
pub fn verify_relocations(rel: &[u8], linked: &[u8]) -> Result<RelocationReport, String> {
    let src = Elf::parse(rel, 1)?;
    let dst = Elf::parse(linked, 2)?;
    let (ti, text) = src.section(".text")?;
    let (di, final_text) = dst.section(".text")?;
    if text.kind != 1
        || text.flags != 6
        || text.size == 0
        || text.align < 2
        || final_text.kind != 1
        || final_text.flags != 6
        || text.size != final_text.size
    {
        return Err("requires identical-sized executable .text".into());
    }
    crate::placement_elf::inspect_linked_elf(
        linked,
        u64::from(final_text.addr),
        u64::from(final_text.addr) + u64::from(final_text.size),
        u32at(linked, 24)?,
    )?;
    for (i, s) in src.sections.iter().enumerate().skip(1) {
        if i != ti && s.flags & 2 != 0 && s.size != 0 {
            return Err("additional source allocated bytes unsupported".into());
        }
        if ![
            ".text",
            ".rela.text",
            ".data",
            ".bss",
            ".comment",
            ".symtab",
            ".strtab",
            ".shstrtab",
        ]
        .contains(&s.name.as_str())
        {
            return Err("unexpected source section".into());
        }
        if s.kind == 9 || (s.kind == 4 && s.name != ".rela.text") {
            return Err("additional relocations unsupported".into());
        }
    }
    for (i, s) in dst.sections.iter().enumerate() {
        if i != di && s.flags & 2 != 0 && s.size != 0 {
            return Err("additional final allocated bytes unsupported".into());
        }
    }
    let (symidx, syms) = src.symbols()?;
    let (_, final_syms) = dst.symbols()?;
    // Reject symbol forms outside this exact non-FDPIC compiler-output profile.
    for s in syms.iter().skip(1) {
        if s.other != 0 {
            return Err("unsupported source symbol visibility".into());
        }
        match s.info & 15 {
            4 if s.info == 4
                && s.section == 0xfff1
                && s.value == 0
                && s.size == 0
                && !s.name.is_empty() => {}
            3 if s.info == 3
                && s.name.is_empty()
                && s.value == 0
                && s.size == 0
                && s.section > 0
                && usize::from(s.section) < src.sections.len() => {}
            2 if (s.info == 2 || s.info == 18) && usize::from(s.section) == ti => {}
            _ => return Err("unsupported or undefined source symbol".into()),
        }
    }
    let mut functions = BTreeMap::new();
    for s in &syms {
        if s.info & 15 == 2 && usize::from(s.section) == ti {
            if s.name.is_empty()
                || s.other != 0
                || u64::from(s.value) + u64::from(s.size) > u64::from(text.size)
            {
                return Err("invalid source function".into());
            }
            if functions.insert(s.name.clone(), s.value).is_some() {
                return Err("duplicate source function".into());
            }
            let matches: Vec<_> = final_syms.iter().filter(|f| f.name == s.name).collect();
            if matches.len() != 1 {
                return Err("missing/duplicate final function".into());
            }
            let f = matches[0];
            if f.info != s.info
                || f.other != s.other
                || f.size != s.size
                || usize::from(f.section) != di
                || u64::from(f.value) != u64::from(final_text.addr) + u64::from(s.value)
            {
                return Err("source/final function provenance mismatch".into());
            }
        }
    }
    let bind = *functions.get("_op1_adapter_bind").ok_or("missing binder")?;
    let allocate = *functions
        .get("_allocate_payload")
        .ok_or("missing allocate thunk")?;
    let release = *functions
        .get("_release_payload")
        .ok_or("missing release thunk")?;
    if allocate == release
        || u64::from(u32at(linked, 24)?) != u64::from(final_text.addr) + u64::from(bind)
    {
        return Err("bridge symbols/entry mismatch".into());
    }
    let (_, rs) = src.section(".rela.text")?;
    if rs.kind != 4
        || rs.entsize != 12
        || rs.size != 60
        || rs.link as usize != symidx
        || rs.info as usize != ti
    {
        return Err("expected five .rela.text records".into());
    }
    let mut expected = src.data(text)?.to_vec();
    let r = src.data(rs)?;
    let mut touched = BTreeSet::new();
    let mut pc = 0;
    let mut halves = BTreeMap::<(u32, u32), usize>::new();
    for p in (0..r.len()).step_by(12) {
        let off = u32at(r, p)?;
        let info = u32at(r, p + 4)?;
        let kind = info & 255;
        let addend = u32at(r, p + 8)? as i32;
        let sym = syms
            .get((info >> 8) as usize)
            .ok_or("relocation symbol out of range")?;
        if usize::from(sym.section) != ti || !off.is_multiple_of(2) {
            return Err("unsupported symbol section or offset".into());
        }
        let relative = i64::from(sym.value) + i64::from(addend);
        if relative < 0 || relative >= i64::from(text.size) {
            return Err("target outside source text".into());
        }
        let value = i64::from(final_text.addr) + relative;
        if !(0..=i64::from(u32::MAX)).contains(&value) {
            return Err("target address overflow".into());
        }
        let (start, width) = if kind == 10 {
            (off.checked_sub(2).ok_or("PCREL offset underflow")?, 4)
        } else if kind == 6 || kind == 7 {
            (off, 2)
        } else {
            return Err("unsupported relocation type".into());
        };
        if u64::from(start) + width > expected.len() as u64 {
            return Err("relocation span outside text".into());
        }
        for at in u64::from(start)..u64::from(start) + width {
            if !touched.insert(at) {
                return Err("overlapping relocations".into());
            }
        }
        let at = start as usize;
        if kind == 10 {
            if sym.name != "_op1_adapter_bind" || sym.info & 15 != 2 || addend != 0 {
                return Err("unexpected PCREL target".into());
            }
            // BFIN relocation offset denotes the instruction's second halfword.
            let delta = value - (i64::from(final_text.addr) + i64::from(start));
            if delta % 2 != 0 || !(-(1i64 << 24)..(1i64 << 24)).contains(&delta) {
                return Err("PCREL displacement out of range".into());
            }
            if expected[at + 1] != 0xe3 {
                return Err("expected CALL opcode".into());
            }
            let encoded = (delta / 2) as u32;
            expected[at] = (encoded >> 16) as u8;
            expected[at + 2..at + 4].copy_from_slice(&(encoded as u16).to_le_bytes());
            pc += 1;
        } else {
            if sym.info & 15 != 3
                || sym.value != 0
                || (relative as u32 != allocate && relative as u32 != release)
            {
                return Err("unexpected immediate target".into());
            }
            // Immediate halfword follows its instruction header; that header stays unchanged.
            if off < 2 || expected[at - 1] != 0xe1 || (expected[at - 2] & 0x40 != 0) != (kind == 7)
            {
                return Err("unexpected half-immediate opcode".into());
            }
            let v = if kind == 7 {
                (value as u32) >> 16
            } else {
                value as u32
            };
            expected[at..at + 2].copy_from_slice(&(v as u16).to_le_bytes());
            *halves.entry((relative as u32, kind)).or_default() += 1;
        }
    }
    if pc != 1
        || halves.len() != 4
        || [allocate, release]
            .iter()
            .any(|v| [6, 7].iter().any(|k| halves.get(&(*v, *k)) != Some(&1)))
    {
        return Err("expected one PCREL and two matched HU/LO pairs".into());
    }
    if expected != dst.data(final_text)? {
        return Err("linked text differs from semantic relocation reconstruction".into());
    }
    Ok(RelocationReport {
        checked_relocations: 5,
        code_base: final_text.addr,
        text_bytes: expected.len(),
    })
}
