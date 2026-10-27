# Typed allocator bridge and placement limits

`firmware/allocator_bridge.h` describes an unbound pair of ordinary one-argument
C allocator functions. `op1_adapter_bind_allocator_bridge` installs context-first
callbacks that explicitly forward the byte count or raw allocation pointer to
those functions. It never casts a function pointer to an incompatible signature,
discovers a firmware address or enables capture.

## What is verified

Native tests check argument forwarding, exact-pointer release, allocation failure,
immutable binding, drain/generation behavior and callback reentry in composition
with the disabled adapter. In the inspected BF524-any code, each forwarding
callback preserves the context in a pointer register, moves the payload from R1
to R0, loads the selected one-argument function and tail-calls it. The caller's
return address and argument area remain in place.

The binding helper reserves its outgoing argument area and passes the fourth
context argument in the required stack slot. This fixes the demonstrated
signature mismatch; it does not establish a particular stock address, a safe
runtime call site, heap lifetime or device compatibility.

## Context lifetime is longer than one reservation

Initialize the bridge context before binding. Both functions, that context and
the original allocator arena must remain valid and unchanged for the entire
bound adapter lifetime. Releasing a reservation does not unbind the adapter or
allow the context to be replaced, reset or destroyed. There is no rebind API.

The context must be disjoint from adapter state, output parameters and allocated
storage. Calls remain externally serialized and subject to the disabled adapter's
trusted-callback contract. Detected mutating reentry poisons the adapter, including
invalid attempts to bind a context during an allocator callback.

## A relocatable bundle is not freely copyable code

The Rust build tool compiles the adapter and bridge separately, then performs a
relocatable link with no startup objects or standard libraries. The combined
object has no named undefined symbols, but retains five relocation entries in the
inspected build: a relative call to the adapter binder and two high/low pairs for
the internal callback addresses.

Consequently, extracting the combined raw text and copying it to an arbitrary
address would lose necessary relocation information. Preserve the relocatable
object until a final placement/linking strategy has been established. The checker
counts relocation records and bounds their storage; it does not apply or validate
their architecture-specific semantics.

The earlier adapter-only object had no relocations. That property must not be
generalized to the new bridge bundle, nor does a synthetic link at a test address
prove that the address is free or executable on an OP-1.

## Placement is still a separate gate

No live firmware code/state address is selected by this bridge. Empty-looking
loader ranges, heap tails and framebuffer padding are not ownership grants.
An allocation provides storage only under its allocator's lifetime contract; it
does not alone establish executable-memory or instruction-cache requirements.

A carveout made by shortening the initial heap table must take effect before
registration and allocations from that range. Shrinking a live arena at main-thread Run entry could
overlap allocations already made while constructing threads. A cold image-level
reservation would require separate proof of loader/init ordering, every relevant
owner, bounds, remaining memory budget and code/cache requirements before it could
become an integration plan.

The current artifact remains permanently disabled, unbound and offline. No stock
image is modified and no target code is executed by the build checks.

## Reviewed build checks

The updated Rust command runs four separate native C test binaries, compiles both
BF524-any units, and produces a combined relocatable object. The bridge's eight
primary scenario groups and six independent groups cover forwarding, failures,
immutable context binding and reentry. The independent groups additionally cover
720 release orders and 512 allocation generations. Earlier core tests remain.

All 73 Rust tests pass in debug/release, with formatting and strict Clippy checks.
Native AddressSanitizer/UndefinedBehaviorSanitizer checks pass with leak detection
disabled; no LeakSanitizer result is claimed. These are offline build and mock
results, not execution of stock functions or a hardware integration test.
