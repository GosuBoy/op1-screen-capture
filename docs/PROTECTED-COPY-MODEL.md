# Protected read windows: offline model

This extends the serialized capture-state model with a bounded copy plan and
explicit read-window lifetime. It does not read pixels, mask interrupts, stop DMA,
lock memory, preserve registers, or implement a firmware adapter. Its offsets are
relative positions in a hypothetical buffer, never device addresses.

## Why model read windows separately?

Invalidating a frame after a writer runs prevents accepting that frame, but does
not by itself make the earlier reads safe. Each read needs its own valid source
lifetime and exclusion/synchronization contract. A possible adapter could acquire
that protection for small chunks, yield between chunks, and discard the entire
attempt if any intervening writer invalidates it.

This model distinguishes three events:

1. Opening a window authorizes a specific bounded interval under asserted source
   readiness and ownership assumptions.
2. Closing that exact window asserts that all reads using it have stopped. Only a
   fully completed window contributes copy progress.
3. Finishing the copy and publishing are separate steps, allowed only after the
   entire planned active-pixel region has completed without invalidation.

The model never stores a source reference. This is not a Rust lifetime proof for
an eventual adapter: that adapter must ensure no source borrow/reference escapes
the protected interval or persists across a yield.

## Ownership rules

- An owner active before admission blocks capture and read-window admission.
- An owner between windows invalidates the entire capture, even if it exits
  before the next window.
- An owner entering while a window is open violates the claimed exclusion
  assumption. The model must fail closed; it must not treat that overlap as safe
  merely because the resulting frame will be rejected.
- Lifecycle loss, DMA fault and bookkeeping failure cannot silently recycle a
  window still in use. Its explicit close/abort is required first.
- Cancellation means copying has actually stopped, not merely that cancellation
  was requested. Published staging also retains its separate consumer lease.
- Stale window identifiers cannot close or advance a newer window.

## Layout and progress

The bounded plan describes packed RGB565 output from a potentially strided source.
Dimensions, row width, source extent and progress arithmetic are validated before
admission. The supplied source extent is an assumption, not a probe of a real
allocation; source and staging still need independently established ownership.
Each window is limited to active pixels in one row: source row padding
is not copied into the packed staging stream. Window-size bounds are software test
limits, not measured safe interrupt-disabled durations on an OP-1.

The original coarse capture mode remains available for earlier model tests. Its
copy-completed event is an external assertion. A bounded-plan capture cannot use
that legacy assertion to bypass its progress or open-window requirements.

## API sequence

1. `CopyLayout::new(width, height, row_stride, source_extent)` checks nonzero
   dimensions up to the crate's software dimension limit, even source stride,
   sufficient asserted source extent and checked range arithmetic.
2. `begin_bounded_capture(layout, max_window_bytes)` reserves the modeled slot.
   The window cap must be positive, even and no larger than `MAX_CHUNK_BYTES`.
3. `begin_read_window(capture_id)` yields the next opaque window, with source and
   packed-staging offsets and length. It never crosses an active row boundary.
4. `end_read_window(window)` asserts full completion and ended source access.
   `abort_read_window(window)` instead asserts stopped partial access and
   invalidates the candidate without crediting progress.
5. Once all windows have completed, `finish_copy(capture_id)` permits the separate
   `publish(capture_id)` transition. The consumer eventually calls `release_frame`.

Cancellation/finish cannot close an open window on the caller's behalf. Window
handles share the checked, nonwrapping, model-instance-local identifier namespace.
Progress queries are accounting information, not permission to access memory.

## Hardware obligations remain open

An actual read window would need a proven synchronization primitive, complete
writer/launch coverage, exclusion of pre-existing or autonomous DMA, stable source
geometry/lifetime, correct cache visibility and a justified time bound. A CPU
interrupt mask alone does not stop DMA or establish all exception behavior. A
full-frame interrupt-disabled copy is not the proposal.

The model can detect illegal supplied event sequences. It cannot prevent an
unobserved real writer, validate physical timing, or establish that every firmware
path emits the required events. Passing tests therefore supports only this
bounded event contract, not flash readiness or successful screen capture.

## Verification scope

At the protected-copy milestone all 69 crate tests passed locally in debug and release, with
formatting and strict all-target Clippy checks passing. There are 19 new checks
beyond the original 50-test suite: 12 focused/domain tests, one window-identifier
exhaustion unit test, and six independently authored adversarial tests preserved
in `tests/protected_copy_adversarial.rs`.

The geometry oracle enumerates 1,080 small layout/window combinations and checks
packed progress, row boundaries, padding exclusion and source ranges. Independent
tests cover interrupted windows, every yield boundary in their finite sample,
stale handles across 256 replacements and checked arithmetic near `usize::MAX`.
The earlier 100,000 bounded event histories remain part of the suite. These are
finite test domains, not exhaustive hardware interleavings or remote CI results.
