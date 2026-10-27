# OP-1 Screen Capture

Research toward capturing the **original Teenage Engineering OP-1** screen over USB.
This is not an OP-1 Field project and is not affiliated with Teenage Engineering.

## Status

**Design and offline feasibility stage. There is no working capture firmware or hardware-ready release here.**

The first goal is one complete frame received and validated on a computer. Continuous
video is a later milestone. Frame rate, USB throughput, live geometry and impact on
audio are not yet established.

## Tooling direction

Prefer **Rust** for the host application, protocol parsing, frame decoding and offline
tests. Device-side language choices depend on verified toolchain and target support;
minimal C or assembly may be necessary. This preference is not a claim that Rust can
currently build the required device firmware.

## Proposed capture design

1. A host explicitly requests one frame; capture is disabled by default.
2. Firmware reserves a pre-acquired, independently owned staging slot.
3. Only at a proven coherent capture boundary, copy validated active pixels into that
   slot. Do not send asynchronously from a borrowed live framebuffer.
4. Return to normal rendering without waiting for USB. Transmit the immutable
   snapshot in bounded, host-paced chunks.
5. The host checks protocol version, session/frame identity, dimensions, pixel format,
   byte order, payload length and integrity before displaying a complete frame.
6. On overload, disconnect, malformed input or timeout, cancel or drop capture rather
   than block normal instrument operation.

The protocol should expose capture operations, not arbitrary memory reads/writes,
firmware installation, resets or calibration commands. Bounds and checked arithmetic
must precede allocations and copies. A checksum detects corruption, not authenticity.

A dedicated vendor-specific bulk interface is a candidate for sustained transfer, not
an established feature. Reusing musical MIDI traffic would need careful capacity,
framing and scheduling analysis. UVC compatibility is deferred until basic capture
works. No transport has been selected as proven safe.

## Proof gaps and next gates

- Establish the capture boundary, live buffer ownership and lifetime, pixel layout,
  dimensions, orientation and coverage of alternate rendering paths.
- Establish staging-memory ownership, allocation failure behavior, cache/DMA
  consistency, bounded copy cost and hook ABI preservation.
- Validate USB resource allocation, descriptors, queue limits, submission errors,
  completion ownership, backpressure and disconnect/reset cleanup.
- Test a host reassembler offline with synthetic frames, including malformed lengths,
  stale/duplicate chunks, truncation, cancellation and reconnect.
- Independently review an exact firmware candidate before separately approved device
  tests. Measure audio/MIDI/rendering impact and actual throughput; do not infer FPS
  from a nominal USB speed or buffer size.

A rendered buffer, published pointer and completed physical display scanout are
different events. None should be treated as an interchangeable synchronization signal.

## Safety and publication scope

Preserve the known-good stock firmware and recovery bootloader. No flashing,
bootloader modification or device test is authorized by this design document.

This repository contains public-safe project documentation. Do not commit proprietary
firmware images, device secrets, private research exports or unrelated personal data.
Any future binary patcher should use owner-supplied inputs and verify the exact
supported baseline before modifying anything.

Future implementation work should use focused branches and reviewed changes. The
host-only prototype does not establish a hardware-ready firmware implementation.

## Host-only Rust prototype

The dependency-free library in `src/lib.rs` parses an experimental synthetic header
and reassembles bounded, ordered RGB565 frames. It exposes complete frames only
after payload-length and CRC validation. It does not communicate with an OP-1.

See [the synthetic protocol and API contract](docs/SYNTHETIC-PROTOCOL.md) for limits,
error recovery, session responsibilities and local test commands. This format is
not a recovered device protocol and may change before any firmware integration.

## Capture safety research

See [capture safety proof status](docs/CAPTURE-SAFETY.md) for the current offline
findings and remaining ownership, phase-boundary and DMA contracts. These findings
do not establish working capture firmware or hardware readiness.

## Offline capture-state model

The serialized Rust model tests fail-closed capture admission, sticky invalidation
and staging-slot ownership using synthetic events. It does not copy pixels or
implement hardware synchronization. See [the model contract and limitations](docs/CAPTURE-MODEL.md).

See [firmware adapter feasibility](docs/ADAPTER-FEASIBILITY.md) for the conditional
stock-allocator reservation route and why queue bookkeeping is not a render lock.

The [protected-copy extension](docs/PROTECTED-COPY-MODEL.md) adds checked row/chunk
progress and explicit read-window lifetime to the offline model. It still performs
no device memory reads or hardware synchronization.

## Disabled adapter scaffold

A freestanding C scaffold now exercises allocator/lease bookkeeping and compiles
as a BF524 relocatable object. It has no capture-enable, pixel-copy or USB path,
no stock bindings and no fixed placement. See [scope and offline Rust build checks](docs/DISABLED-ADAPTER.md).

The [typed allocator bridge](docs/ALLOCATOR-BRIDGE.md) adapts single-argument
allocator signatures without raw firmware addresses or incompatible pointer
casts. The combined object retains relocations and is not a loadable image.
