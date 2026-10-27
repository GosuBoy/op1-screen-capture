# Offline capture-state model

This milestone models the admission and lifetime of a single still-frame capture.
It does **not** capture pixels, access a device, synchronize actual threads, or
prove a safe OP-1 firmware hook. The model is separate from the synthetic host
reassembler and is not a recovered firmware interface.

## What the model is intended to check

- Startup and loss of hardware trust reject capture until a new explicit modeled
  normalization cycle completes.
- Active owners, including suspended writers, cannot be mistaken for quiescence.
- Only one capture or published staging slot can occupy the slot at a time.
- A writer that starts and ends during a capture still invalidates that attempt.
- Restart, geometry/lifetime changes and DMA faults cannot resurrect old attempts.
- Completing a copy is not itself permission to publish.
- Published staging data has independent ownership. Later rendering cannot change
  an already-owned snapshot; release is an explicit modeled event.
- Stale capture events cannot complete or cancel a newer attempt; exhausted
  identifiers and inconsistent ownership bookkeeping fail closed.

The event stream is serialized. A model event asserts that a hypothetical adapter
has already performed the corresponding observation or ownership operation. It
is not an implementation of that operation. In particular, modeled normalization
cannot drain an actual DMA engine or prove that no suspended owner can resume.

## Event API and recovery policy

- `begin_owner` acquires a unique modeled lease for a CPU or DMA owner.
  Nested entries get distinct leases. Suspension does not release a lease.
  There are eight identity labels per kind and at most sixteen simultaneous
  leases; these are test-model bounds, not device hardware counts.
- `begin_normalization`, four ordered `assume_step` calls, then
  `finish_normalization` model a new trust-establishment cycle. The assumptions
  are owner exclusion, engine draining, fresh status and coherent pinned source
  lifetime. They must be established by an eventual target adapter, not guessed.
- `begin_capture`, `finish_copy`, then `publish` represent a successful attempt.
  Copy completion is an asserted event, not validation of actual pixels or length.
- `cancel_capture` means the copying activity has stopped and its staging access
  is finished. A mere cancellation request is not sufficient to emit this event.
- `release_frame` means the published snapshot consumer has finished, or its
  cancellation has been confirmed. Restart does not substitute for this event.

A normal owner invalidates an in-progress or staged attempt, even if the owner
exits before publication. A fault or lifecycle/geometry/lifetime change also
revokes trust. Existing owner leases are retained across these changes; a late
DMA exit cannot restore trust. An invalidated candidate must be consumed or
cancelled before a new normalization cycle can start.

Unknown owner labels, exhausted lease capacity, unmatched owner exits and ID
exhaustion permanently poison admission within that model instance. Other leases
remain held until explicitly ended. Stale capture/normalization identifiers do
not alter a newer attempt; stale owner exits intentionally poison the history
rather than silently accepting inconsistent ownership bookkeeping.

All handle IDs share a checked, nonwrapping counter and belong to one model
instance's event stream. They are not security capabilities and must not be
replayed into another instance. A new model has a new logical namespace, not a
mechanism for safely resetting real hardware or recycling outstanding leases.
The lifecycle event models changes within a live run; it does not preserve heap
allocations through an actual reboot, power loss or firmware replacement. A new
run needs fresh initialization and transport-session handling.

## Test scope

Focused tests cover normalization ordering/interruption, nested and suspended
ownership, duplicate exits, capacity limits, sticky invalidation, late DMA exit,
copy completion, publication/release, stale events and identifier exhaustion.

The bounded reference test enumerates all 100,000 length-five histories over ten
abstract events, comparing 500,000 transitions from a modeled Ready state.
Normalization is a complete macro event in that exploration; partial cycles,
DMA owner cases, stale identifiers and numerical limits are tested separately.
It is a finite regression oracle, not an unbounded proof or a hardware scheduler.

Six independently authored adversarial tests are preserved in
`tests/capture_model_adversarial.rs`. They exercise every normalization prefix,
1,000 stale-ticket lifecycle replacements, both forward and reverse release of
sixteen mixed owner leases, and slot retention across faults and invalidation.

For the original capture-state milestone, all 50 tests passed in both debug and release builds: 23 model
checks (including the bounded exploration) and 27 unchanged parser tests.
Formatting and strict all-target Clippy checks also passed. These are local offline
results, not a claim of remote CI or device validation.

## Prerequisites outside this model

Before using any analogous policy on hardware, separately establish:

1. Complete interception of every relevant writer and lifecycle transition, with
   invalidation before the first write.
2. Atomic admission and ownership transitions, not a check followed by an
   unprotected store.
3. Safe source lifetime and geometry throughout any attempted read, including
   attempts whose output is later discarded.
4. A memory-access mechanism legal under the target language and platform. Sticky
   invalidation does not repair a C/Rust data race or reading freed memory.
5. Actual DMA ownership, outstanding-transfer draining, status freshness, error
   handling and cache coherence for the supported hardware revision.
6. Independently owned, bounded staging storage and a copy-time budget that does
   not interfere with the instrument.
7. Transport completion and cancellation ownership before recycling staging.

Passing model tests establishes properties only under the modeled event contract.
It does not establish writer coverage, hardware timing, display scanout completion,
an available staging address, or any frame rate. Tests must report their bounded
domain rather than claim all possible firmware interleavings were proved.

## Protected-copy extension

The [bounded read-window extension](PROTECTED-COPY-MODEL.md) adds explicit per-row
progress and window lifetime. Its bounded mode cannot bypass progress by using the
coarse mode's copy-completed assertion. Both modes remain symbolic event models,
not firmware synchronization implementations.

## Running checks

The crate has no third-party dependencies. From the repository root:

```sh
cargo test --offline
cargo test --offline --release
cargo fmt --all -- --check
cargo clippy --offline --all-targets -- -D warnings
```

Existing synthetic parser tests remain part of the regression suite.
