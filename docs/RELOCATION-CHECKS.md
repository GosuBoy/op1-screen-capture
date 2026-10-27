# Bounded Blackfin relocation-result checks

The offline verifier now compares the supplied combined relocatable adapter
object with the actual final linked ELF. It reconstructs the entire expected
`.text` and compares every byte, not just the absence of relocation records.
`check_placement` invokes this verifier after its existing geometry, SHA-256 and
ELF-envelope checks. Capture remains permanently disabled.

## Scope

This is deliberately specific to the current adapter/bridge shape: one unchanged
read-only `.text`, one `R_BFIN_PCREL24` call to `_op1_adapter_bind`, and two
`R_BFIN_HUIMM16`/`R_BFIN_LUIMM16` pairs for `_allocate_payload` and
`_release_payload`. Relocation order may vary. Missing, duplicated, overlapping,
unsupported or differently targeted records are rejected. Additional allocated
sections or linker changes to the text layout are unsupported, not guessed.

The verifier parses the actual source REL relocation records, symbols and signed
addends. It checks translation of every source function into the final symbol
table, entrypoint identity, target boundaries and opcode shape. Relocated fields
are reconstructed while all other bytes, including opcode bits, must remain
identical to the source. This detects an unrelated or modified source object
paired with a final artifact unless their changes are mutually consistent.

Input identity is a separate trust boundary. `check_placement` hashes the exact
source/final bytes against its supplied manifest; the standalone command below
checks their relationship only. Neither command authenticates a manifest or
proves where its inputs originated. A coherently changed source, symbols and
linked result can pass; this is not a signature, trusted-build attestation,
firmware authenticity check or semantic proof of the C program.

## Equations and primary source

The semantics were checked against GNU Binutils 2.44, matching the existing
linker's reported version, from the [official GNU source archive](https://ftp.gnu.org/gnu/binutils/binutils-2.44.tar.xz).
Archive SHA-256: `ce2017e059d63e67ddb9240e9d4ec49c2893605035cd60e92ad53177f4377237`.
`bfd/elf32-bfin.c` SHA-256:
`ed8f5c5e4ac58313cfad22af912c1a5a14553e612af7144ea0ca5da22a6eeb28`.
`include/elf/bfin.h` SHA-256:
`21a022ecfb13aef59835543b9f30a3c2cc441e6842b6a4a7524512ec82c6acc6`.
No GNU implementation code is vendored here; the checker implements the bounded
format relationship independently. Source/version agreement does not prove the
installed linker binary was built from that exact unmodified source.

Let `B` be the final `.text` base, `S` the source section-relative symbol value,
`A` the signed RELA addend, and `r` the source relocation offset.

- HUIMM16: write the upper 16 bits of `B + S + A` at `r`.
- LUIMM16: write the lower 16 bits of `B + S + A` at `r`.
- PCREL24: its instruction starts at `r - 2`, not `r`. Calculate the even
  displacement `D = (B + S + A) - (B + r - 2)`, then the signed halfword
  displacement `D / 2`. Store its upper 8 bits in the first instruction
  halfword's low byte, preserve that halfword's opcode high byte, and store
  the lower 16 bits in the second halfword.

Relevant upstream definitions: `bfin.h` relocation IDs 6, 7 and 10;
`elf32-bfin.c` HOWTO entries around lines 514–582, and
`bfin_final_link_relocate` around lines 1309–1360. Only in-text targets and the
current ordinary non-FDPIC encodings are accepted; this is not a complete
Blackfin relocator or a claim about arbitrary relocation overflow behavior.

## Reproduce

With the already installed Rust and Blackfin tools on PATH:

```sh
cargo test --offline
cargo test --offline --release
cargo fmt --check
cargo clippy --offline --all-targets -- -D warnings
sh scripts/check_relocations_synthetic.sh
```

Override `BFIN_CC` and `BFIN_LD` with paths to existing tools if necessary. The
script builds the existing four native tests and target REL object, then links
that exact object with `ld -n` (no page-padding) at six artificial bases. They exercise lower and upper address
halves, a 16-bit carry and a high 32-bit address. These addresses are **not device
placement proposals**. It checks each link and explicitly runs the ignored
real-artifact corruption test with that pair. The ordinary test suite separately
uses small synthetic ELF fixtures; the real-artifact test is ignored by default
because it requires the target toolchain outputs.

For one already generated pair:

```sh
cargo run --offline --bin check_relocations -- original.o final.elf
```

Success proves only the bounded source-to-linked-byte relationship. No target
instructions are executed. Boot/startup ownership, reservation ordering, cache
visibility, retained heap capacity, integrated hooks, writer coverage, timing
and hardware capture safety remain unresolved independent gates.
