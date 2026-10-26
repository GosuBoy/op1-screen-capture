# Capture safety: proof status and required contracts

This is a public-safe summary of ongoing offline analysis, not a device interface
specification or an implementation guide. **There is no working capture firmware
and no hardware proof of safe capture.** The Rust prototype exercises synthetic
host-side reassembly only.

## Findings so far

- A display busy flag is not a global lock. Observing it alone cannot establish
  exclusive ownership of a framebuffer or exclude every concurrent writer.
- Display handling follows a recurring lifecycle. A state observed in one pass
  does not establish ownership or safety across later passes.
- Normal phase entry and exit have been identified in offline analysis, but the
  effects of calls within those phases remain incompletely established. A phase
  boundary is therefore not yet a proven coherent capture point.
- DMA-backed rendering or any future DMA-based copy requires exclusive ownership, a proven idle
  engine and fresh completion/error status associated with the new operation.
  An idle-looking flag or an old completion indication is not sufficient.

These findings narrow the questions; they do not prove that a capture hook, spare
DMA resource or safe snapshot lifetime exists.

## Contracts still required

1. **Coherent source:** identify every writer and alternate path; establish source
   lifetime, geometry, layout and the exact interval during which pixels cannot
   change. Separate rendered content from physical display scanout.
2. **Phase effects:** account for relevant callees, callbacks, interrupts and error
   paths before treating normal entry or exit as a synchronization boundary.
3. **DMA ownership:** establish arbitration, exclusive acquisition, idle checks,
   status clearing and fresh completion attribution. Specify cache visibility,
   transfer bounds, cancellation, timeout, error recovery and resource release.
   These are proof obligations, not verified properties of the device.
4. **Snapshot lifetime:** pre-acquire independently owned bounded staging storage;
   keep it immutable until transmission completion or confirmed cancellation.
   Allocation or queue failure must drop capture rather than block the instrument.
5. **Transport lifetime:** establish USB submission/completion ownership, bounded
   queues, backpressure and disconnect/reset cleanup without disrupting audio,
   MIDI or rendering. No transport or throughput target is proven here.
6. **Validation gate:** independently review an exact candidate and its failure
   paths before any separately approved hardware experiment. Offline models and
   synthetic tests cannot substitute for measured device behavior.

No flashing, device traffic or bootloader modification is authorized by this
document. Private source exports, firmware bytes, addresses and research artifacts
are intentionally excluded from this public summary.
