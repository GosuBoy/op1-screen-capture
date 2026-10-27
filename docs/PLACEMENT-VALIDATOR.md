# Offline placement manifest validator

This is a fail-closed **structural checker**, not a firmware builder or a certificate
of safe physical placement. It never chooses a device address, writes firmware,
loads target code, touches USB, or enables capture. Every address in the example
is artificial. No usable stock placement manifest is supplied.

## Run

With Rust and the standard `sha256sum` utility already installed:

```sh
cargo test --offline
cargo run --offline --bin check_placement -- \
  proposed.manifest canonical-input.bin combined-relocatable.o final-linked.elf
```

The command hashes the exact input bytes read into memory, compares all three
SHA-256 identities to the manifest, validates the proposed geometry, and checks
the actual linked ELF envelope. The canonical and relocatable bytes are hashed,
not parsed by this command. A matching hash binds inputs to a manifest; it does
not authenticate that manifest or prove that its heap descriptor, ABI, hook
inventory or evidence statements were derived correctly. Use independently
reviewed, trusted input identities. Empty/placeholder or unrelated input files
must never be treated as a meaningful integration proof even if their hashes
were intentionally entered into a synthetic manifest.

Exit 2 means rejection, including missing evidence-reference fields. Exit 0 means
only that the structural checks passed and all reference fields were supplied.
It is **never permission to build/install/execute a device image**. The command
prints `NOT DEVICE READY` on successful lint. Nonempty reference strings are not
proof that the referenced reports exist, are correct, or cover the chosen image.

## Manifest format

`examples/placement-synthetic.manifest` documents the strict version-1 `key=value`
format. Empty lines and whole-line `#` comments are allowed. Duplicate, unknown,
missing or malformed keys are rejected. Numbers are unsigned decimal or `0x`
hexadecimal. Digests are 64 lowercase hexadecimal characters. No implicit defaults
or address selection is performed.

The cold reservation must be declared `before_first_heap_registration`.
`usable_start` is an explicitly reviewed allocator-derived usable boundary, not
inferred from loader gaps. All ranges are half-open, nonempty, power-of-two
aligned, checked for arithmetic overflow, and bounded by the 32-bit address
space. An exclusive end of exactly 2^32 is valid. The cut excludes a strict heap
tail; code and two immediately adjacent guards must be disjoint and inside it.
The entry must be instruction-aligned and within the declared final image.

`final_image_bytes` must cover the final linked module, hooks, trampolines and
initialization path, not just the current adapter/bridge text. The inventory
string is a declaration, not automated source/CFG verification. ELF section
checks bind the declared envelope to actual linked bytes; they cannot detect an
omitted hook that never entered the link.

State and staging are **runtime allocation requirements**, not fixed addresses:
92-byte adapter, 8-byte immutable bridge context, and a nonzero staging request.
The checker requires alignment at least four and adds worst-case alignment slack
for three allocations plus a positive explicit allocator-overhead budget. These
and the positive retained-stock budget must fit in the retained usable arena.
The exact budgets remain review inputs, not measurements of worst-case runtime
heap usage. Fragmentation, allocator metadata and stock peak/stack usage require
separate capacity evidence. Allocation failure must preserve stock operation.

The runtime contract declaration requires zero-initialized state, bridge lifetime
beyond every reservation, immutable bridge bindings, disjoint allocations, ordinary
serialized access, and fail-closed failures. These are implementation obligations;
the checker does not observe a target allocator or grant executable allocation.

## Evidence and remaining gates

The following `evidence.NAME` fields identify reports for independent review:

- `boot_owners`: predecessor loader, startup scratch and reservation ordering.
- `alias_dma_owners`: overlapping arenas, indirect/static owners and DMA aliases.
- `cache_execution`: instruction mapping, visibility, synchronization and cache.
- `stock_capacity`: allocator metadata, fragmentation, stock heap and stack budget.
- `startup_hook`: actual hook ABI, final code inventory, state lifetime and failure path.
- `link_provenance`: trusted final link inputs, linker script/map, symbols, and
  architecture-specific relocation/disassembly review.

Link envelope inspection rejects unsupported or malformed ELF and residual
relocation/undefined-symbol cases within its documented narrow format. An ELF
with no relocation records is **not proof that a linker applied every original
relocation correctly**. Matching hashes plus a structurally valid artifact still
require semantic link provenance; neither arbitrary metadata counts nor clearing
relocation sections can satisfy that independent gate. Hardware writer coverage,
read exclusion, timing and safe capture remain separate unresolved requirements.

The synthetic example deliberately omits all evidence references and uses dummy
hashes. It is a rejection fixture, not a starting device image. Tests exercise
geometry and malformed plans; passing tests do not validate physical placement.

### Deliberately narrow linked-ELF format

The checker accepts ELF32 little-endian Blackfin `ET_EXEC` with a complete symbol
table, resolved non-null symbols, executable entry in a file-backed executable
section, and matching load-segment/allocated-section coverage. Its current subset
is read-only, fully file-backed code/rodata without unmapped load padding. Writable
or `NOBITS` target state must not be smuggled into this code artifact: state remains
a separate runtime requirement. Residual relocation sections, dynamic loading,
unsupported table/count encodings, inconsistent mappings and out-of-range symbols
are rejected rather than guessed. A default linker script can produce unsupported
layout/symbols even for legitimate code; rejection is not a diagnosis that the
compiler or linker is defective. Use an independently reviewed explicit script.

These intentionally narrow constraints fit the disabled freestanding bundle.
They do not implement architecture relocation semantics or a general ELF loader.

## Reproducible real-toolchain smoke test

With the already approved GNU Blackfin compiler, linker and readelf on PATH:

```sh
sh scripts/check_placement_synthetic.sh
```

Alternatively set `BFIN_CC`, `BFIN_LD` and `BFIN_READELF` to their full paths.
This reruns all four native adapter/bridge binaries and the BF524-any compile,
links through `examples/placement-synthetic.ld` at an artificial address, then
checks the actual linked ELF with the manifest CLI. It also verifies rejection
of the original relocatable object (with the matching digest, so ELF validation
is exercised) and missing-evidence plans. The canonical-input file and every
review-reference string are explicitly synthetic. All generated artifacts stay
under `target/placement-synthetic`; nothing is installed or executed on a device.
The script requires Rust, a native C compiler, `sha256sum`, `awk`, `sed`, and the
existing Blackfin tools. It installs nothing. The fixed 1024-byte synthetic
reservation intentionally fails if a future final bundle outgrows it.
