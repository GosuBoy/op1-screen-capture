//! Experimental, synthetic host-side frame reassembly. Not an OP-1 wire protocol.
//! No USB, device access, firmware patching, or arbitrary memory commands.
#![forbid(unsafe_code)]

pub mod capture_model;

pub const HEADER_LEN: usize = 36;
pub const CHUNK_HEADER_LEN: usize = 24;
pub const MAX_DIMENSION: u16 = 512;
pub const MAX_FRAME_BYTES: usize = 512 * 512 * 2;
pub const MAX_CHUNK_BYTES: usize = 4096;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Error {
    HeaderLength,
    Magic,
    Version,
    PixelFormat,
    Reserved,
    Dimensions,
    PayloadLength,
    WrongSession,
    StaleFrame,
    Busy,
    Allocation,
    NoFrame,
    ChunkLength,
    WrongFrame,
    Offset,
    Incomplete,
    Checksum,
}

/// Metadata of a validated synthetic header. Rows are tightly packed RGB565 LE.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Header {
    pub width: u16,
    pub height: u16,
    pub payload_len: usize,
    pub session: u64,
    pub frame_id: u64,
    pub crc32: u32,
}

impl Header {
    /// Parse exactly one header, rejecting unsupported fields before allocation.
    pub fn parse(bytes: &[u8]) -> Result<Self, Error> {
        if bytes.len() != HEADER_LEN {
            return Err(Error::HeaderLength);
        }
        if &bytes[..4] != b"OPCF" {
            return Err(Error::Magic);
        }
        if bytes[4] != 1 {
            return Err(Error::Version);
        }
        if bytes[5] != 1 {
            return Err(Error::PixelFormat);
        }
        if bytes[6..8] != [0, 0] {
            return Err(Error::Reserved);
        }
        let width = u16::from_le_bytes([bytes[8], bytes[9]]);
        let height = u16::from_le_bytes([bytes[10], bytes[11]]);
        if width == 0 || height == 0 || width > MAX_DIMENSION || height > MAX_DIMENSION {
            return Err(Error::Dimensions);
        }
        let expected = usize::from(width)
            .checked_mul(usize::from(height))
            .and_then(|n| n.checked_mul(2))
            .ok_or(Error::PayloadLength)?;
        let declared = u32_at(bytes, 12);
        let payload_len = usize::try_from(declared).map_err(|_| Error::PayloadLength)?;
        if payload_len != expected || payload_len > MAX_FRAME_BYTES {
            return Err(Error::PayloadLength);
        }
        Ok(Self {
            width,
            height,
            payload_len,
            session: u64_at(bytes, 16),
            frame_id: u64_at(bytes, 24),
            crc32: u32_at(bytes, 32),
        })
    }
}

/// Only constructed by a successful finish: complete, length-checked, CRC-checked.
#[derive(Debug, PartialEq, Eq)]
pub struct Frame {
    header: Header,
    pixels: Vec<u8>,
}
impl Frame {
    pub fn header(&self) -> &Header {
        &self.header
    }
    pub fn pixels(&self) -> &[u8] {
        &self.pixels
    }
}

#[derive(Debug)]
struct Pending {
    header: Header,
    pixels: Vec<u8>,
}

/// One active capture, ordered chunks, and monotonic IDs within one session.
/// Caller supplies a session identity and must not reuse it across reconnects.
/// This is not authentication. Caller implements timeouts by invoking cancel().
#[derive(Debug)]
pub struct Reassembler {
    session: u64,
    last_frame_id: Option<u64>,
    pending: Option<Pending>,
}
impl Reassembler {
    pub fn new(session: u64) -> Self {
        Self {
            session,
            last_frame_id: None,
            pending: None,
        }
    }
    pub fn is_active(&self) -> bool {
        self.pending.is_some()
    }

    /// Busy or invalid begin leaves any current frame unchanged. An accepted ID
    /// is consumed even if that capture is subsequently cancelled or malformed.
    pub fn begin(&mut self, bytes: &[u8]) -> Result<(), Error> {
        if self.pending.is_some() {
            return Err(Error::Busy);
        }
        let header = Header::parse(bytes)?;
        if header.session != self.session {
            return Err(Error::WrongSession);
        }
        if self
            .last_frame_id
            .is_some_and(|last| header.frame_id <= last)
        {
            return Err(Error::StaleFrame);
        }
        let mut pixels = Vec::new();
        pixels
            .try_reserve_exact(header.payload_len)
            .map_err(|_| Error::Allocation)?;
        self.last_frame_id = Some(header.frame_id);
        self.pending = Some(Pending { header, pixels });
        Ok(())
    }

    /// Accept one entire chunk packet (not an arbitrary transport fragment).
    /// Every chunk error aborts the active capture; no partial frame is exposed.
    pub fn push_chunk(&mut self, packet: &[u8]) -> Result<(), Error> {
        let result = self.append_chunk(packet);
        if result.is_err() {
            self.pending = None;
        }
        result
    }
    fn append_chunk(&mut self, packet: &[u8]) -> Result<(), Error> {
        let pending = self.pending.as_mut().ok_or(Error::NoFrame)?;
        if packet.len() < CHUNK_HEADER_LEN {
            return Err(Error::ChunkLength);
        }
        let length = usize::from(u16::from_le_bytes([packet[20], packet[21]]));
        if length == 0 || length > MAX_CHUNK_BYTES || packet.len() != CHUNK_HEADER_LEN + length {
            return Err(Error::ChunkLength);
        }
        if packet[22..24] != [0, 0] {
            return Err(Error::Reserved);
        }
        if u64_at(packet, 0) != self.session {
            return Err(Error::WrongSession);
        }
        if u64_at(packet, 8) != pending.header.frame_id {
            return Err(Error::WrongFrame);
        }
        let offset = usize::try_from(u32_at(packet, 16)).map_err(|_| Error::Offset)?;
        if offset != pending.pixels.len() {
            return Err(Error::Offset);
        }
        let end = offset.checked_add(length).ok_or(Error::PayloadLength)?;
        if end > pending.header.payload_len {
            return Err(Error::PayloadLength);
        }
        // Capacity for the entire declared payload was reserved in begin().
        pending
            .pixels
            .extend_from_slice(&packet[CHUNK_HEADER_LEN..]);
        Ok(())
    }

    /// End this capture, discarding it on incomplete data or checksum mismatch.
    pub fn finish(&mut self) -> Result<Frame, Error> {
        let pending = self.pending.take().ok_or(Error::NoFrame)?;
        if pending.pixels.len() != pending.header.payload_len {
            return Err(Error::Incomplete);
        }
        if crc32(&pending.pixels) != pending.header.crc32 {
            return Err(Error::Checksum);
        }
        Ok(Frame {
            header: pending.header,
            pixels: pending.pixels,
        })
    }
    /// Discard capture (for cancellation, timeout, or disconnect). ID stays consumed.
    pub fn cancel(&mut self) -> bool {
        self.pending.take().is_some()
    }
}

// Internal decoding helpers are called only after fixed minimum-length checks.
fn u32_at(b: &[u8], i: usize) -> u32 {
    u32::from_le_bytes([b[i], b[i + 1], b[i + 2], b[i + 3]])
}
fn u64_at(b: &[u8], i: usize) -> u64 {
    u64::from_le_bytes([
        b[i],
        b[i + 1],
        b[i + 2],
        b[i + 3],
        b[i + 4],
        b[i + 5],
        b[i + 6],
        b[i + 7],
    ])
}

/// CRC-32/ISO-HDLC (reflected IEEE): polynomial 0xEDB88320, init/final XOR all ones.
/// Integrity only, not authentication. Caller should bound untrusted input lengths.
pub fn crc32(bytes: &[u8]) -> u32 {
    let mut crc = u32::MAX;
    for &byte in bytes {
        crc ^= u32::from(byte);
        for _ in 0..8 {
            crc = (crc >> 1) ^ (0xEDB88320 & 0u32.wrapping_sub(crc & 1));
        }
    }
    !crc
}

pub mod placement;
pub mod placement_elf;

pub mod relocation;
