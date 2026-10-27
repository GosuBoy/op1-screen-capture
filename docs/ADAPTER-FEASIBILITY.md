# Firmware adapter feasibility: reservation and exclusion

This is an offline feasibility checkpoint for original OP-1 stock firmware, not a
patch, callable ABI specification, or permission to test hardware. The public
repository deliberately excludes proprietary bytes and private source exports.

## Staging: a concrete allocation route, with unresolved prerequisites

Static analysis identifies a stock general-heap allocation wrapper and matching
exact-pointer release path. This provides a candidate for reserving an owned
staging allocation instead of guessing unused framebuffer, stack or USB memory.

The proposed policy is:

1. In a separately established ordinary-thread initialization context, validate
   the required packed-frame size using checked arithmetic and a bounded geometry
   contract. Account for any justified alignment padding before allocation.
2. Attempt one reservation. Allocation failure leaves capture disabled. Nominal
   arena size does not establish current free space or a safe memory budget.
3. Retain the original allocation pointer for release, even if a usable interior
   pointer is aligned separately. Never pass an interior pointer to the stock
   deallocator.
4. Do not allocate or free in a render hook or interrupt handler: the inspected
   allocator contains free-list scans and runtime lock hooks. Neither bounded
   latency nor ISR suitability follows from the wrapper being callable.
5. Within a live firmware run, and only while the allocator arena remains valid,
   retain the reservation through display lifecycle
   changes until copying and any snapshot consumer have actually finished.
   Changing modes or observing a reset flag is not consumer completion.

The inspected allocator supports eight-byte block alignment under valid heap
state. That is not proof of a larger cache-line or DMA alignment requirement.
Lazy-initialization guards suggest an ordinary reuse path, but do not prove that
the heap can never be reset or corrupted by other paths. ABI, lock-hook binding,
execution context, memory budget and lifetime across all relevant transitions
remain implementation gates.

An actual reboot, power loss or firmware/address-space replacement is different
from an in-process display lifecycle change. Old heap pointers cannot be retained
across it. A new run starts untrusted and needs fresh initialization and transport
session handling. The offline model does not provide persistent memory or reboot
recovery.

## Source exclusion: queue bookkeeping is not a render lock

An apparent pair of operations around graphics dispatch resolves to queue-record
consumption and a small bookkeeping clear, not a mutex held across rendering.
Consequently, using that pair as the snapshot's exclusion mechanism is rejected.

Likewise, a display busy flag, a callback notification, a phase landmark or a
semaphore wake cannot by itself exclude every framebuffer writer or prove DMA
completion. A new adapter still needs an actual enforcement mechanism covering
ordinary rendering, alternate paths, asynchronous writers and lifecycle changes.

The model's owner and normalization events are obligations for that adapter, not
evidence that such hooks already exist. Merely observing events after writes would
be insufficient. Rejecting a torn frame after copying also does not make an unsafe
concurrent memory read legal.

## Narrower setup candidate

The ordinary main-thread binding can be traced through its configured creator,
runtime entry and virtual Run dispatch. Its entry prefix precedes the recurring
application lifecycle loop. This nominates a place for one reservation per Run
invocation, rather than allocation in a display callback or every lifecycle pass.

This is not a once-per-machine guarantee. Thread recreation, destruction, call
ABI, stack headroom and ownership of added code/state still need explicit handling.
A bounded exact-address reference census found allocator lookup/registration
operations, not an ordinary heap-head/count reset. Computed aliases, bulk writes
and other arena metadata remain outside that census; it is not a lifetime proof.

## First-frame direction: CPU copy, reject rather than repair

Prefer a CPU-copy design that never starts, resets, reprograms or cancels shared
DMA for capture. If source readiness cannot be established, drop the capture
request rather than force the engine into a desired state. This removes the
capture transfer's own DMA programming and recovery obligations, not the need to
exclude stock DMA writes to the source.

A finite proposed owner-hook set covers these known categories:

- Ordinary render entry and source-context rebinding.
- Display service, including inline/special writes and direct text rendering.
- Existing DMA writer entries before their first register mutation.
- Diagnostic rendering outside the ordinary graphics path.
- Lifecycle, callback and source-selection changes.

The categories are a partial coverage certificate, not proof that every incoming
edge, computed target, interior entry or aliased write is intercepted. A stock
DMA helper returning is also not automatically an owner-release event: completion
freshness and actual draining must be justified independently.

A possible way to make source reads safe is a sequence of small protected copy
chunks: validate ownership and pinned geometry inside each protected read window,
then restore normal scheduling between chunks. No source borrow/reference may
escape its protected interval or persist across a yield. Any writer between chunks
invalidates the entire attempt. Final publication must atomically recheck validity.
This is a proposal, not an implemented synchronization primitive. It requires
complete writer/launch coverage, precise ordering, exclusion of autonomous DMA,
known exception/interrupt behavior and a justified worst-case chunk time. A full
frame copied with interrupts disabled is not the proposed solution. Volatile reads
or a later checksum do not substitute for those requirements.

## Incoming-edge audit: stronger witnesses, still partial coverage

A bounded canonical-byte audit distinguishes branch candidates aligned with a
linear record decode from matching halfwords embedded in other instructions. It also resolves a
limited set of adjacent immediate pointer loads followed by indirect calls, and
known callback registrations. These checks confirm why a direct-call-only search
would miss relevant service and diagnostic paths.

The audit supports keeping the direct diagnostic writer in the proposed hook set,
not merely its more common diagnostic wrapper. Removing operand/continuation
false positives makes the candidate ledger more accurate, but does not establish
whole-program reachability, complete indirect dispatch, absence of arbitrary
interior entry, or absence of aliased memory writes. Capture must not be enabled
by treating this partial certificate as complete writer coverage.

## Decision

- Continue with an explicit reservation-and-ownership design rather than a fixed
  spare-memory address or implicit queue lock.
- Do not emit target code that calls the allocator or copies pixels until its
  context, lifetime, geometry and exclusion prerequisites have been established.
- Keep the existing DMA handoff and cache-coherence gates open. Choosing a CPU
  copy does not eliminate source writes performed by an existing DMA owner.
- No capture hook, memory reservation, hardware throughput or audio impact has
  been validated on a device.

See [the capture-state model](CAPTURE-MODEL.md) and
[the remaining safety contracts](CAPTURE-SAFETY.md).
