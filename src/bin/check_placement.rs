//! Offline lint only. Never writes firmware or enables capture.
use op1_screen_capture::{placement, placement_elf};
use std::{
    env, fs,
    io::Write,
    process::{Command, ExitCode, Stdio},
};
fn digest(bytes: &[u8]) -> Result<String, String> {
    let mut child = Command::new("sha256sum")
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .spawn()
        .map_err(|e| format!("sha256sum: {e}"))?;
    child
        .stdin
        .take()
        .ok_or("missing hash input")?
        .write_all(bytes)
        .map_err(|e| e.to_string())?;
    let output = child.wait_with_output().map_err(|e| e.to_string())?;
    if !output.status.success() {
        return Err("cannot hash input".into());
    }
    let text = String::from_utf8(output.stdout).map_err(|_| "invalid hash output")?;
    let hash = text.split_whitespace().next().ok_or("empty hash output")?;
    if hash.len() != 64 || !hash.bytes().all(|b| b.is_ascii_hexdigit()) {
        return Err("invalid hash output".into());
    }
    Ok(hash.to_owned())
}
fn run() -> Result<(), String> {
    let args: Vec<_> = env::args().collect();
    if args.len() != 5 {
        return Err(
            "usage: check_placement MANIFEST CANONICAL_INPUT RELOCATABLE_MODULE LINKED_ELF".into(),
        );
    }
    let m = placement::parse(&fs::read_to_string(&args[1]).map_err(|e| e.to_string())?)?;
    // Hash exactly the bytes consumed, never a second pathname read.
    let canonical = fs::read(&args[2]).map_err(|e| e.to_string())?;
    let module = fs::read(&args[3]).map_err(|e| e.to_string())?;
    let bytes = fs::read(&args[4]).map_err(|e| e.to_string())?;
    let actual = placement::InputHashes {
        canonical: digest(&canonical)?,
        module: digest(&module)?,
        linked: digest(&bytes)?,
    };
    let report = placement::validate(&m, &actual)?;
    placement_elf::inspect_linked_elf(
        &bytes,
        m.code.start,
        m.code.start + m.final_image_bytes,
        m.entry,
    )?;
    println!(
        "STRUCTURAL CHECKS PASS: manifest geometry, input identities and linked ELF envelope."
    );
    println!(
        "Retained usable heap: {}; runtime budget: {}; excluded tail: {} bytes.",
        report.retained_usable_bytes, report.runtime_budget, report.excluded_bytes
    );
    if !report.missing_evidence.is_empty() {
        return Err(format!(
            "incomplete plan: missing review references {:?}",
            report.missing_evidence
        ));
    }
    println!("All review reference fields supplied; references are NOT verified evidence.");
    println!("NOT DEVICE READY: relocation semantics/link provenance, boot ownership, cache execution, capacity and runtime contracts require independent physical integration review. Capture remains disabled.");
    Ok(())
}
fn main() -> ExitCode {
    match run() {
        Ok(()) => ExitCode::SUCCESS,
        Err(e) => {
            eprintln!("REJECTED: {e}. No device readiness granted.");
            ExitCode::from(2)
        }
    }
}
