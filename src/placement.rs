//! Address-neutral, offline placement checks. Never grants device readiness.
use std::collections::BTreeMap;

pub const ADDRESS_END: u64 = 1u64 << 32;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Region {
    pub start: u64,
    pub bytes: u64,
    pub alignment: u64,
}
impl Region {
    pub fn end(self) -> Result<u64, String> {
        if self.bytes == 0
            || !self.alignment.is_power_of_two()
            || self.alignment > ADDRESS_END
            || !self.start.is_multiple_of(self.alignment)
        {
            return Err("empty or misaligned region / invalid alignment".into());
        }
        let end = self
            .start
            .checked_add(self.bytes)
            .ok_or("region overflow")?;
        if self.start >= ADDRESS_END || end > ADDRESS_END {
            return Err("region outside 32-bit address space".into());
        }
        Ok(end)
    }
    fn inside(self, start: u64, end: u64) -> Result<bool, String> {
        Ok(self.start >= start && self.end()? <= end)
    }
}

#[derive(Debug, Clone)]
pub struct Manifest {
    pub canonical_sha256: String,
    pub module_sha256: String,
    pub linked_sha256: String,
    pub heap: Region,
    pub usable_start: u64,
    pub cut: u64,
    pub retained_stock_budget: u64,
    pub code: Region,
    pub guard_before: Region,
    pub guard_after: Region,
    pub final_image_bytes: u64,
    pub entry: u32,
    pub state_bytes: u64,
    pub bridge_bytes: u64,
    pub staging_bytes: u64,
    pub runtime_alignment: u64,
    pub allocation_overhead_budget: u64,
    pub pre_registration: bool,
    pub final_hook_inventory: bool,
    pub runtime_contract: bool,
    pub evidence: BTreeMap<String, String>,
}

/// Supplied by the caller after hashing the actual input files, not copied from the manifest.
#[derive(Debug, Clone)]
pub struct InputHashes {
    pub canonical: String,
    pub module: String,
    pub linked: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StructuralReport {
    pub excluded_bytes: u64,
    pub retained_usable_bytes: u64,
    pub runtime_budget: u64,
    /// References are recorded for review; their existence/content/truth is not verified.
    pub missing_evidence: Vec<&'static str>,
}

pub const EVIDENCE_KEYS: [&str; 6] = [
    "boot_owners",
    "alias_dma_owners",
    "cache_execution",
    "stock_capacity",
    "startup_hook",
    "link_provenance",
];
fn hash(s: &str) -> bool {
    s.len() == 64
        && s.bytes()
            .all(|b| b.is_ascii_hexdigit() && !b.is_ascii_uppercase())
}

pub fn validate(m: &Manifest, actual: &InputHashes) -> Result<StructuralReport, String> {
    for (expected, found) in [
        (&m.canonical_sha256, &actual.canonical),
        (&m.module_sha256, &actual.module),
        (&m.linked_sha256, &actual.linked),
    ] {
        if !hash(expected) || !hash(found) || expected != found {
            return Err("input SHA-256 mismatch or malformed digest".into());
        }
    }
    let end = m.heap.end()?;
    if !m.pre_registration {
        return Err("reservation must precede first heap registration".into());
    }
    if m.usable_start < m.heap.start
        || m.usable_start >= m.cut
        || m.cut >= end
        || !m.cut.is_multiple_of(m.heap.alignment)
    {
        return Err("invalid heap usable start or cold cut".into());
    }
    if !m.usable_start.is_multiple_of(m.heap.alignment) {
        return Err("unaligned usable heap start".into());
    }
    if !m.final_hook_inventory || m.final_image_bytes == 0 || m.final_image_bytes > m.code.bytes {
        return Err("final hook/module inventory absent or exceeds code reservation".into());
    }
    let regions = [m.guard_before, m.code, m.guard_after];
    for region in regions {
        if !region.inside(m.cut, end)? {
            return Err("reservation lies outside excluded tail".into());
        }
    }
    for i in 0..regions.len() {
        for j in i + 1..regions.len() {
            if regions[i].start < regions[j].end()? && regions[j].start < regions[i].end()? {
                return Err("reserved regions overlap".into());
            }
        }
    }
    if m.guard_before.end()? != m.code.start || m.code.end()? != m.guard_after.start {
        return Err("guards must directly bracket code".into());
    }
    if u64::from(m.entry) < m.code.start
        || u64::from(m.entry) >= m.code.start + m.final_image_bytes
        || !m.entry.is_multiple_of(2)
    {
        return Err("entry outside final image or not instruction aligned".into());
    }
    if m.allocation_overhead_budget == 0
        || !m.runtime_contract
        || m.state_bytes != 92
        || m.bridge_bytes != 8
        || m.staging_bytes == 0
        || !m.runtime_alignment.is_power_of_two()
        || m.runtime_alignment < 4
        || m.runtime_alignment > ADDRESS_END
    {
        return Err("runtime state/staging contract or target ABI is invalid".into());
    }
    // Three independent runtime allocations: worst-case alignment slack per allocation.
    let slack = m
        .runtime_alignment
        .checked_sub(1)
        .and_then(|n| n.checked_mul(3))
        .ok_or("runtime alignment overflow")?;
    let runtime = m
        .state_bytes
        .checked_add(m.bridge_bytes)
        .and_then(|n| n.checked_add(m.staging_bytes))
        .and_then(|n| n.checked_add(slack))
        .and_then(|n| n.checked_add(m.allocation_overhead_budget))
        .ok_or("runtime budget overflow")?;
    let usable = m.cut - m.usable_start;
    if m.retained_stock_budget == 0
        || m.retained_stock_budget
            .checked_add(runtime)
            .ok_or("total budget overflow")?
            > usable
    {
        return Err("retained heap budget insufficient".into());
    }
    let missing_evidence = EVIDENCE_KEYS
        .into_iter()
        .filter(|key| m.evidence.get(*key).is_none_or(|s| s.trim().is_empty()))
        .collect();
    Ok(StructuralReport {
        excluded_bytes: end - m.cut,
        retained_usable_bytes: usable,
        runtime_budget: runtime,
        missing_evidence,
    })
}

/// Strict version-1 key=value format. No implicit defaults, duplicate or unknown keys.
pub fn parse(text: &str) -> Result<Manifest, String> {
    let mut map = BTreeMap::new();
    for line in text.lines() {
        let line = line.trim();
        if line.is_empty() || line.starts_with('#') {
            continue;
        }
        let (key, value) = line.split_once('=').ok_or("expected key=value")?;
        let key = key.trim().to_owned();
        let value = value.trim().to_owned();
        if map.insert(key, value).is_some() {
            return Err("duplicate key".into());
        }
    }
    fn take(m: &mut BTreeMap<String, String>, k: &str) -> Result<String, String> {
        m.remove(k).ok_or_else(|| format!("missing {k}"))
    }
    fn num(m: &mut BTreeMap<String, String>, k: &str) -> Result<u64, String> {
        let s = take(m, k)?;
        let parsed = if let Some(s) = s.strip_prefix("0x") {
            u64::from_str_radix(s, 16)
        } else {
            s.parse()
        };
        parsed.map_err(|_| format!("invalid number {k}"))
    }
    fn region(m: &mut BTreeMap<String, String>, name: &str) -> Result<Region, String> {
        Ok(Region {
            start: num(m, &format!("{name}.start"))?,
            bytes: num(m, &format!("{name}.bytes"))?,
            alignment: num(m, &format!("{name}.alignment"))?,
        })
    }
    if take(&mut map, "version")? != "1" {
        return Err("unsupported manifest version".into());
    }
    let mut manifest = Manifest {
        canonical_sha256: take(&mut map, "canonical_sha256")?,
        module_sha256: take(&mut map, "module_sha256")?,
        linked_sha256: take(&mut map, "linked_sha256")?,
        heap: region(&mut map, "heap")?,
        usable_start: num(&mut map, "usable_start")?,
        cut: num(&mut map, "cut")?,
        retained_stock_budget: num(&mut map, "retained_stock_budget")?,
        code: region(&mut map, "code")?,
        guard_before: region(&mut map, "guard_before")?,
        guard_after: region(&mut map, "guard_after")?,
        final_image_bytes: num(&mut map, "final_image_bytes")?,
        entry: u32::try_from(num(&mut map, "entry")?).map_err(|_| "entry out of range")?,
        state_bytes: num(&mut map, "state_bytes")?,
        bridge_bytes: num(&mut map, "bridge_bytes")?,
        staging_bytes: num(&mut map, "staging_bytes")?,
        runtime_alignment: num(&mut map, "runtime_alignment")?,
        allocation_overhead_budget: num(&mut map, "allocation_overhead_budget")?,
        pre_registration: take(&mut map, "reservation_stage")? == "before_first_heap_registration",
        final_hook_inventory: take(&mut map, "image_inventory")?
            == "final_module_hooks_trampolines_init",
        runtime_contract: take(&mut map, "runtime_contract")?
            == "zeroed_state_immutable_bridge_lifetime_disjoint_serialized_fail_closed",
        evidence: BTreeMap::new(),
    };
    for key in EVIDENCE_KEYS {
        if let Some(value) = map.remove(&format!("evidence.{key}")) {
            manifest.evidence.insert(key.into(), value);
        }
    }
    if !map.is_empty() {
        return Err(format!(
            "unknown keys: {:?}",
            map.keys().collect::<Vec<_>>()
        ));
    }
    Ok(manifest)
}
