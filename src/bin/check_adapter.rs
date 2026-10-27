//! Offline build checks only. Never executes the Blackfin object or accesses a device.
#![forbid(unsafe_code)]

use std::{env, fs, path::Path, process::Command};

fn u16_at(bytes: &[u8], at: usize) -> Result<u16, String> {
    let slice = bytes.get(at..at + 2).ok_or("truncated ELF field")?;
    Ok(u16::from_le_bytes([slice[0], slice[1]]))
}

fn u32_at(bytes: &[u8], at: usize) -> Result<u32, String> {
    let slice = bytes.get(at..at + 4).ok_or("truncated ELF field")?;
    Ok(u32::from_le_bytes(slice.try_into().expect("four bytes")))
}

/// A deliberately narrow compiler-output envelope check, not a general ELF loader,
/// instruction validator, linker, relocation resolver or target ABI proof.
#[derive(Debug, PartialEq, Eq)]
struct ObjectSummary {
    sections: usize,
    relocations: usize,
}

fn inspect_object(bytes: &[u8]) -> Result<ObjectSummary, String> {
    if bytes.len() < 52 || bytes.get(..7) != Some(b"\x7fELF\x01\x01\x01") {
        return Err("expected little-endian ELF32 version 1".into());
    }
    if u16_at(bytes, 16)? != 1 || u16_at(bytes, 18)? != 106 || u32_at(bytes, 20)? != 1 {
        return Err("expected a relocatable Blackfin object".into());
    }
    if u32_at(bytes, 24)? != 0 || u16_at(bytes, 44)? != 0 {
        return Err("object must not specify an entry point or program headers".into());
    }
    let table = usize::try_from(u32_at(bytes, 32)?).map_err(|_| "section offset overflow")?;
    let count = usize::from(u16_at(bytes, 48)?);
    if u16_at(bytes, 40)? != 52 || u16_at(bytes, 46)? != 40 || count < 2 {
        return Err("unsupported ELF header/section layout".into());
    }
    let end = table
        .checked_add(count.checked_mul(40).ok_or("section count overflow")?)
        .ok_or("section table overflow")?;
    if table < 52 || end > bytes.len() {
        return Err("section table outside object".into());
    }
    let mut symbol_tables = 0;
    let mut relocations: usize = 0;
    for index in 0..count {
        let header = table + index * 40;
        let kind = u32_at(bytes, header + 4)?;
        let offset = usize::try_from(u32_at(bytes, header + 16)?).map_err(|_| "offset overflow")?;
        let size = usize::try_from(u32_at(bytes, header + 20)?).map_err(|_| "size overflow")?;
        if kind != 8 && kind != 0 && offset.checked_add(size).is_none_or(|end| end > bytes.len()) {
            return Err("section outside object".into());
        }
        if kind == 4 || kind == 9 {
            let entry_size = if kind == 4 { 12 } else { 8 };
            if u32_at(bytes, header + 36)? as usize != entry_size || size % entry_size != 0 {
                return Err("unsupported relocation table envelope".into());
            }
            relocations = relocations
                .checked_add(size / entry_size)
                .ok_or("relocation count overflow")?;
        }
        if kind == 2 {
            symbol_tables += 1;
            if u32_at(bytes, header + 36)? != 16 || size % 16 != 0 {
                return Err("unsupported ELF symbol table".into());
            }
            for position in (offset..offset + size).step_by(16) {
                if u32_at(bytes, position)? != 0 && u16_at(bytes, position + 14)? == 0 {
                    return Err(
                        "unexpected named undefined symbol; inspect target dependencies".into(),
                    );
                }
            }
        }
    }
    if symbol_tables == 0 {
        return Err("compiler object lacks a symbol table".into());
    }
    Ok(ObjectSummary {
        sections: count,
        relocations,
    })
}

fn run(program: &std::ffi::OsStr, args: &[&std::ffi::OsStr], root: &Path) -> Result<(), String> {
    let status = Command::new(program)
        .args(args)
        .current_dir(root)
        .status()
        .map_err(|e| format!("could not run {}: {e}", program.to_string_lossy()))?;
    if !status.success() {
        return Err(format!("{} failed: {status}", program.to_string_lossy()));
    }
    Ok(())
}

fn check() -> Result<(), String> {
    let args: Vec<_> = env::args_os().skip(1).collect();
    let host_only = match args.as_slice() {
        [] => false,
        [arg] if arg == "--host-only" => true,
        _ => return Err("usage: cargo run --bin check_adapter -- [--host-only]".into()),
    };
    let root = Path::new(env!("CARGO_MANIFEST_DIR"));
    let output = root.join("target/adapter-check");
    fs::create_dir_all(&output).map_err(|e| e.to_string())?;
    let cc = env::var_os("CC").unwrap_or_else(|| "cc".into());
    for (source, name) in [
        ("firmware/test_adapter.c", "adapter-native-tests"),
        ("firmware/test_allocator_bridge.c", "bridge-native-tests"),
        (
            "firmware/test_allocator_bridge_adversarial.c",
            "bridge-native-adversarial",
        ),
        (
            "firmware/test_adapter_adversarial.c",
            "adapter-native-adversarial",
        ),
    ] {
        let native = output.join(format!("{name}{}", env::consts::EXE_SUFFIX));
        let flags = [
            "-std=c11",
            "-O2",
            "-Wall",
            "-Wextra",
            "-Werror",
            "-pedantic",
            "firmware/adapter.c",
            "firmware/allocator_bridge.c",
            source,
            "-o",
        ];
        let mut native_args: Vec<&std::ffi::OsStr> =
            flags.iter().map(std::ffi::OsStr::new).collect();
        native_args.push(native.as_os_str());
        run(&cc, &native_args, root)?;
        run(native.as_os_str(), &[], root)?;
    }
    if host_only {
        println!("Host-only checks passed; BF524 compile deliberately not run.");
        return Ok(());
    }
    let bfin = env::var_os("BFIN_CC").unwrap_or_else(|| "bfin-elf-gcc".into());
    let object = output.join("adapter-bf524.o");
    let core = output.join("adapter-core-bf524.o");
    let bridge = output.join("allocator-bridge-bf524.o");
    for (source, destination) in [
        ("firmware/adapter.c", &core),
        ("firmware/allocator_bridge.c", &bridge),
    ] {
        let flags = [
            "-std=c11",
            "-Os",
            "-ffreestanding",
            "-fno-builtin",
            "-fno-common",
            "-mcpu=bf524-any",
            "-Wall",
            "-Wextra",
            "-Werror",
            "-c",
            source,
            "-o",
        ];
        let mut target_args: Vec<&std::ffi::OsStr> =
            flags.iter().map(std::ffi::OsStr::new).collect();
        target_args.push(destination.as_os_str());
        run(&bfin, &target_args, root)?;
    }
    let link_flags = [
        "-mcpu=bf524-any",
        "-nostdlib",
        "-fno-use-linker-plugin",
        "-r",
    ];
    let mut link_args: Vec<&std::ffi::OsStr> =
        link_flags.iter().map(std::ffi::OsStr::new).collect();
    link_args.extend([
        core.as_os_str(),
        bridge.as_os_str(),
        std::ffi::OsStr::new("-o"),
        object.as_os_str(),
    ]);
    run(&bfin, &link_args, root)?;
    let bytes = fs::read(&object).map_err(|e| e.to_string())?;
    let summary = inspect_object(&bytes)?;
    println!(
        "BF524-any compile passed: {} bytes, {} sections, {} relocation entries, no named undefined symbols.",
        bytes.len(), summary.sections, summary.relocations
    );
    if summary.relocations != 0 {
        println!("Relocations retained: raw text is not a loadable firmware payload.");
    }
    println!("Relocatable object only; no target execution, placement, ABI integration or hardware validation.");
    Ok(())
}

fn main() {
    if let Err(error) = check() {
        eprintln!("adapter check failed: {error}");
        std::process::exit(1);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn fixture() -> Vec<u8> {
        let mut b = vec![0; 148];
        b[..7].copy_from_slice(b"\x7fELF\x01\x01\x01");
        for (at, value) in [(16, 1u16), (18, 106), (40, 52), (46, 40), (48, 2)] {
            b[at..at + 2].copy_from_slice(&value.to_le_bytes());
        }
        for (at, value) in [
            (20, 1u32),
            (32, 52),
            (96, 2),
            (108, 132),
            (112, 16),
            (128, 16),
        ] {
            b[at..at + 4].copy_from_slice(&value.to_le_bytes());
        }
        b
    }

    #[test]
    fn accepts_minimal_rel_envelope() {
        assert_eq!(
            inspect_object(&fixture()),
            Ok(ObjectSummary {
                sections: 2,
                relocations: 0
            })
        );
    }

    #[test]
    fn rejects_wrong_class_type_machine_or_entry() {
        for at in [4, 5, 6, 16, 18, 20, 24, 44] {
            let mut b = fixture();
            b[at] ^= 3;
            assert!(inspect_object(&b).is_err(), "offset {at}");
        }
    }

    #[test]
    fn rejects_truncated_sections_and_bad_symbols() {
        let b = fixture();
        for length in 0..b.len() {
            assert!(inspect_object(&b[..length]).is_err());
        }
        let mut b = fixture();
        b[132] = 1;
        assert!(inspect_object(&b).is_err());
        let mut b = fixture();
        b[128] = 8;
        assert!(inspect_object(&b).is_err());
    }
    #[test]
    fn counts_relocations_without_applying_them() {
        let mut b = fixture();
        b.resize(200, 0);
        b[48..50].copy_from_slice(&3u16.to_le_bytes());
        for (at, value) in [(108, 172u32), (136, 4), (148, 188), (152, 12), (168, 12)] {
            b[at..at + 4].copy_from_slice(&value.to_le_bytes());
        }
        assert_eq!(
            inspect_object(&b),
            Ok(ObjectSummary {
                sections: 3,
                relocations: 1
            })
        );
        b[168] = 8;
        assert!(inspect_object(&b).is_err());
    }
}
