use std::{env, fs, process::ExitCode};
fn run() -> Result<(), String> {
    let a: Vec<_> = env::args().collect();
    if a.len() != 3 {
        return Err("usage: check_relocations ORIGINAL_REL LINKED_EXEC".into());
    }
    let r = op1_screen_capture::relocation::verify_relocations(
        &fs::read(&a[1]).map_err(|e| e.to_string())?,
        &fs::read(&a[2]).map_err(|e| e.to_string())?,
    )?;
    println!("Verified {} relocation results and {} text bytes at synthetic/supplied base {:#x}. Input authentication and hardware readiness remain separate.",r.checked_relocations,r.text_bytes,r.code_base);
    Ok(())
}
fn main() -> ExitCode {
    match run() {
        Ok(()) => ExitCode::SUCCESS,
        Err(e) => {
            eprintln!("REJECTED: {e}");
            ExitCode::from(2)
        }
    }
}
