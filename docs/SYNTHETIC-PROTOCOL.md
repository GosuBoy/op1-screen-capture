# Experimental synthetic frame envelope v1

This is a host-only test format we designed, **not** a recovered OP-1 protocol,
not a firmware interface, and not a stable interoperability commitment. It has
no device transport, byte-stream packet framer, GUI or capture implementation.
The caller passes exactly one header or one whole chunk packet per API call.
A future transport adapter must bound its own receive buffers before buffering
an untrusted packet and enforce deadlines. This library owns at most one active
frame of up to 524,288 bytes; the caller can retain completed frames and is
responsible for bounding that retention.

All integers are little-endian. Only tightly packed RGB565 little-endian pixels
are supported. Row stride is width * 2; orientation is unspecified/native order.
No padding bytes are transmitted. Dimensions 1..=512 are synthetic host limits,
not asserted device geometry. CRC-32/ISO-HDLC covers pixel payload only and is
not authentication; header changes to another valid geometry are not detected
by this checksum. Session/frame IDs provide consistency, not hostile-peer trust.

## Frame header: exactly 36 bytes

| Offset | Bytes | Meaning |
|---|---:|---|
| 0 | 4 | ASCII OPCF |
| 4 | 1 | version = 1 |
| 5 | 1 | format = 1 (RGB565 LE) |
| 6 | 2 | reserved, zero |
| 8 | 2 | width |
| 10 | 2 | height |
| 12 | 4 | payload length, exactly width * height * 2 |
| 16 | 8 | session ID |
| 24 | 8 | frame ID |
| 32 | 4 | pixel CRC-32/ISO-HDLC |

## Chunk: exactly 24 + length bytes

| Offset | Bytes | Meaning |
|---|---:|---|
| 0 | 8 | session ID |
| 8 | 8 | frame ID |
| 16 | 4 | payload byte offset |
| 20 | 2 | length, 1..=4096 |
| 22 | 2 | reserved, zero |
| 24 | length | pixel bytes |

Chunk boundaries may split pixels. Ordered contiguous chunks only: offset must
match bytes already accepted. Duplicates, gaps, reordering, stale identities,
invalid lengths, and payload overrun abort the capture. No retransmission or
reordering window is implemented. A new capture needs a greater frame ID.

## State and failure contract

- Construct `Reassembler` with the connection's session ID. The caller must use a
  fresh session on reconnect; recreating the object with an old session removes
  replay protection. Session uniqueness is not generated or enforced here.
- `begin` accepts one validated header and fallibly reserves payload storage.
  Invalid/busy/allocation-failed begin leaves state unchanged. A successful begin
  consumes the frame ID, even if capture is later cancelled, incomplete or invalid.
- Frame IDs strictly increase (gaps allowed). `u64::MAX` exhausts the session;
  no wraparound is accepted. Zero is a valid initial ID.
- `push_chunk` copies an entire valid chunk. Any error drops the active capture.
  No partial pixels are exposed. Successful chunk acceptance is not device or
  USB acknowledgement.
- `finish` consumes the active capture. Only exact length and matching CRC return
  an immutable `Frame`. Incomplete/checksum failures discard it. A successful frame
  cannot be modified through its accessors.
- `cancel` drops an active capture and reports whether one existed. Use it for
  explicit cancellation, timeout or disconnect. There is no clock, USB queue or
  device teardown here, so this does not prove device-side quiescence.

There is no unsafe code or third-party dependency. CRC known-vector and synthetic
state-machine tests exercise host behavior only. These tests cannot establish
framebuffer ownership, display geometry, firmware ABI safety, USB throughput,
audio impact or hardware readiness. Allocation failure is reported but is not
force-injected by the current test suite.

## Local checks

With an existing Rust toolchain:

```
cargo test --offline
cargo fmt --check
cargo clippy --offline --all-targets -- -D warnings
```

Tests construct packets independently from the parser's field readers. No firmware
or device-derived fixtures are included.
