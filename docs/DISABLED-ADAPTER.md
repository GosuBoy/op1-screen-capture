# Permanently disabled adapter prototype

The freestanding C code in `firmware/` is an offline integration scaffold, not
patched firmware. It provides reservation and ownership bookkeeping but **cannot
admit capture**: `op1_adapter_capture_allowed` always returns zero, and there is no
enable, pixel-copy, USB, MMIO or firmware-installation API.

Disabled does not mean side-effect-free if someone calls the scaffold: explicit
reservation/release calls invoke supplied allocator callbacks and update caller-
owned state. In this repository only native mock callbacks are executed. The
Blackfin build is a relocatable object and is never executed by the check tool.

## Contract

- Zero-initialize an instance once. Do not reset, copy or directly edit a live
  instance. Public fields provide freestanding storage, not mutation permission.
- Bind allocator callbacks once. They are unbound integration interfaces, not
  recovered stock function addresses.
- Reserve at most one bounded raw allocation. Zero/excessive requests fail;
  allocation failure does not trigger a retry loop or fallback address.
- Writer and synthetic consumer leases are bounded, nested and instance-local,
  with checked nonwrapping IDs. They are bookkeeping, not real interception or
  synchronization, and a rejected entry does not prove that a writer stopped.
- Withdrawal retains storage and leases. Reclamation requires an explicit drain
  confirmation and no tracked writer/consumer leases, then releases the exact
  original allocation pointer once. Unknown ownership poisons the instance and
  deliberately prevents reclamation.
- All calls require external serialization and a separately proved allowed
  thread context. Callbacks must return normally and preserve their ABI. This is
  not an ISR-safe allocator, lock, nonlocal-exit recovery system or heap-lifetime
  guarantee. Confirmed drain remains an assertion about untracked platform users.

Detected mutating callback reentry poisons the instance. If allocation then
returns a pointer, it is retained and the operation reports poisoning rather than
success. If the free callback has already released memory, reentry cannot undo
that external effect: the pointer is cleared exactly once after normal callback
return, and future operations remain poisoned. This is detection, not recovery
from arbitrary callback behavior.

There is no source/staging pointer API for copying pixels in this milestone.
The software capacity and lease-count bounds are not measured device capacity,
hardware owner counts or real-time budgets. Physical reboot/image replacement
ends the instance and allocator generation; stale pointers must not be retained.

## Concrete ABI findings

Representative GNU Blackfin compiler probes reserve a 12-byte outgoing register
home area; a fourth argument is placed at stack offset 12. The inspected stock
allocation/release wrappers take their sole payload argument in R0. This
prototype's callback signatures instead take context in R0 and payload in R1.
**Casting a stock allocator address to the callback type is invalid.** A separate
verified bridge is required. The [typed bridge](ALLOCATOR-BRIDGE.md) now supplies
the signature adaptation, but still installs no stock addresses or live hook.

The nominated main-thread entry has a stack-frame setup before the recurring
lifecycle call. Hooking before or after that setup has different preservation
requirements. A proposed entry location is not proof of overwrite safety,
relocation, stack headroom, register liveness or callable ABI integration.

No fixed code/state placement has been established. The build therefore stops at
an unlinked relocatable object. It does not guess free LDR gaps, reuse framebuffer
padding, overwrite stock globals, or claim that an available heap block is an
executable code placement.

## Reproducible offline checks

Rust remains the host tooling language. The dependency-free checker compiles and
runs native C mock tests, then compiles but does not execute the BF524 object:

```sh
cargo test --offline
cargo run --offline --bin check_adapter
```

It expects `cc` and an already installed `bfin-elf-gcc` on PATH. Set `CC` and/or
`BFIN_CC` to executable paths if needed; these variables accept one executable,
not shell commands with arguments. Use a trusted host compiler for `CC` and a
trusted Blackfin cross-compiler for `BFIN_CC`; the checker does not authenticate
those executables. The tool does not download or install tools.
To explicitly skip cross-compilation:

```sh
cargo run --offline --bin check_adapter -- --host-only
```

The target build uses `-mcpu=bf524-any` with the verified GNU 13.2.0 toolchain.
This compiler setting enables its all-known-revision workaround configuration
(`__SILICON_REVISION__` is `0xffff` in the inspected compiler). It is **not** a
physical OP-1 silicon/ROM identification or a promise of hardware compatibility.
Plain `-mcpu=bf524` in that compiler defaults to revision `0x0002`; that default
must not be confused with a device measurement.

Outputs stay under `target/adapter-check/`. The Rust checker verifies a narrow
ELF envelope: little-endian ELF32, Blackfin, relocatable, no entry/program headers,
bounded section/symbol-table storage and no named undefined symbols. This is not
a general ELF parser, instruction validator, relocation resolver or ABI proof.
The current adapter-plus-bridge bundle retains relocations and needs a future
final placement/linking step; the checker counts but does not resolve them.

## Original adapter-only snapshot results

- All 72 Rust tests pass in debug and release, including three checker-envelope
  tests; formatting and strict all-target Clippy pass.
- Both native C binaries pass: 12 primary tests and six independent scenario
  groups, including 720 lease-release orders and 256 allocation generations.
- Native AddressSanitizer/UndefinedBehaviorSanitizer checks pass with leak
  detection disabled. LeakSanitizer could not run under the executor's ptrace
  environment; no leak-sanitizer result is claimed.
- The inspected BF524-any object has 944 bytes of text, no data/BSS, no undefined
  symbols or relocations. Caller-owned target state is 92 bytes, alignment 4,
  in this compiler configuration. These are build properties, not installed
  memory placement, total stack cost or hardware timing.

The check tool deliberately makes a narrower claim than the independent object
review: its ELF envelope check does not validate instruction semantics. Source
filename paths can change non-code symbol/string-table bytes between equivalent
object builds. Only source and tests are published here, not a firmware image.

## Remaining gates

This scaffold does not connect to the symbolic protected-copy model or implement
its read windows on hardware. Actual stock bindings, code/state placement,
complete writer coverage, source-DMA exclusion, synchronization, cache visibility,
heap lifetime, timing and an independently reviewed exact firmware candidate all
remain prerequisites before separately approved device testing.
