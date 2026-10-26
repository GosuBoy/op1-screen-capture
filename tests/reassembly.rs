use op1_screen_capture::*;

fn header(session: u64, frame: u64, w: u16, h: u16, payload: &[u8]) -> Vec<u8> {
    let mut b = b"OPCF".to_vec();
    b.extend_from_slice(&[1, 1, 0, 0]);
    b.extend_from_slice(&w.to_le_bytes());
    b.extend_from_slice(&h.to_le_bytes());
    b.extend_from_slice(&(payload.len() as u32).to_le_bytes());
    b.extend_from_slice(&session.to_le_bytes());
    b.extend_from_slice(&frame.to_le_bytes());
    b.extend_from_slice(&crc32(payload).to_le_bytes());
    b
}
fn chunk(session: u64, frame: u64, offset: u32, payload: &[u8]) -> Vec<u8> {
    let mut b = session.to_le_bytes().to_vec();
    b.extend_from_slice(&frame.to_le_bytes());
    b.extend_from_slice(&offset.to_le_bytes());
    b.extend_from_slice(&(payload.len() as u16).to_le_bytes());
    b.extend_from_slice(&[0, 0]);
    b.extend_from_slice(payload);
    b
}
fn active() -> Reassembler {
    let mut r = Reassembler::new(7);
    r.begin(&header(7, 1, 2, 1, &[0; 4])).unwrap();
    r
}
#[test]
fn standard_crc_vectors() {
    assert_eq!(crc32(b"123456789"), 0xcbf43926);
    assert_eq!(crc32(b""), 0);
}
#[test]
fn complete_frame_in_odd_sized_chunks() {
    let pixels = [0, 248, 224, 7, 31, 0, 255, 255];
    let mut r = Reassembler::new(7);
    r.begin(&header(7, 0, 2, 2, &pixels)).unwrap();
    r.push_chunk(&chunk(7, 0, 0, &pixels[..3])).unwrap();
    r.push_chunk(&chunk(7, 0, 3, &pixels[3..])).unwrap();
    let frame = r.finish().unwrap();
    assert_eq!(frame.pixels(), pixels);
    assert_eq!(frame.header().width, 2);
    assert!(!r.is_active());
    assert_eq!(r.finish(), Err(Error::NoFrame));
}
#[test]
fn truncated_and_trailing_headers_rejected() {
    let b = header(7, 1, 1, 1, &[0; 2]);
    for n in 0..HEADER_LEN {
        assert_eq!(Header::parse(&b[..n]), Err(Error::HeaderLength));
    }
    let mut extra = b;
    extra.push(0);
    assert_eq!(Header::parse(&extra), Err(Error::HeaderLength));
}
#[test]
fn fixed_header_fields_rejected() {
    for (offset, value, error) in [
        (0, 0, Error::Magic),
        (4, 2, Error::Version),
        (5, 2, Error::PixelFormat),
        (6, 1, Error::Reserved),
        (7, 1, Error::Reserved),
    ] {
        let mut b = header(7, 1, 1, 1, &[0; 2]);
        b[offset] = value;
        assert_eq!(Header::parse(&b), Err(error));
    }
}
#[test]
fn zero_and_excessive_dimensions_rejected() {
    for (w, h) in [(0, 1), (1, 0), (513, 1), (1, 513), (u16::MAX, u16::MAX)] {
        assert_eq!(
            Header::parse(&header(7, 1, w, h, &[])),
            Err(Error::Dimensions)
        );
    }
}
#[test]
fn length_mismatch_and_huge_length_rejected() {
    let mut b = header(7, 1, 1, 1, &[0; 4]);
    assert_eq!(Header::parse(&b), Err(Error::PayloadLength));
    b[12..16].copy_from_slice(&u32::MAX.to_le_bytes());
    assert_eq!(Header::parse(&b), Err(Error::PayloadLength));
}
#[test]
fn maximum_frame_is_accepted_bounded() {
    let pixels = vec![0xa5; MAX_FRAME_BYTES];
    let mut r = Reassembler::new(7);
    r.begin(&header(7, 1, 512, 512, &pixels)).unwrap();
    for (i, p) in pixels.chunks(MAX_CHUNK_BYTES).enumerate() {
        r.push_chunk(&chunk(7, 1, (i * MAX_CHUNK_BYTES) as u32, p))
            .unwrap();
    }
    assert_eq!(r.finish().unwrap().pixels(), pixels);
}
#[test]
fn begin_wrong_session_does_not_consume_id() {
    let mut r = Reassembler::new(7);
    assert_eq!(
        r.begin(&header(8, 1, 1, 1, &[0; 2])),
        Err(Error::WrongSession)
    );
    r.begin(&header(7, 1, 1, 1, &[0; 2])).unwrap();
}
#[test]
fn busy_preserves_active_frame() {
    let mut r = active();
    assert_eq!(r.begin(&[]), Err(Error::Busy));
    r.push_chunk(&chunk(7, 1, 0, &[0; 4])).unwrap();
    assert!(r.finish().is_ok());
}
#[test]
fn cancel_and_timeout_policy_consumes_id() {
    let mut r = active();
    assert!(r.cancel());
    assert!(!r.cancel());
    assert_eq!(
        r.begin(&header(7, 1, 2, 1, &[0; 4])),
        Err(Error::StaleFrame)
    );
    r.begin(&header(7, 2, 2, 1, &[0; 4])).unwrap();
}
#[test]
fn finished_id_cannot_be_replayed() {
    let mut r = active();
    r.push_chunk(&chunk(7, 1, 0, &[0; 4])).unwrap();
    r.finish().unwrap();
    for id in [0, 1] {
        assert_eq!(
            r.begin(&header(7, id, 2, 1, &[0; 4])),
            Err(Error::StaleFrame)
        );
    }
}
#[test]
fn max_id_does_not_wrap() {
    let mut r = Reassembler::new(7);
    r.begin(&header(7, u64::MAX, 1, 1, &[0; 2])).unwrap();
    r.cancel();
    assert_eq!(
        r.begin(&header(7, 0, 1, 1, &[0; 2])),
        Err(Error::StaleFrame)
    );
}
#[test]
fn truncated_chunk_prefixes_abort() {
    let packet = chunk(7, 1, 0, &[0; 4]);
    for n in 0..CHUNK_HEADER_LEN {
        let mut r = active();
        assert_eq!(r.push_chunk(&packet[..n]), Err(Error::ChunkLength));
        assert!(!r.is_active());
    }
}
#[test]
fn truncated_trailing_empty_oversized_chunks_abort() {
    let p = chunk(7, 1, 0, &[0; 4]);
    let mut extra = p.clone();
    extra.push(0);
    for packet in [
        p[..p.len() - 1].to_vec(),
        extra,
        chunk(7, 1, 0, &[]),
        chunk(7, 1, 0, &vec![0; 4097]),
    ] {
        let mut r = active();
        assert_eq!(r.push_chunk(&packet), Err(Error::ChunkLength));
        assert_eq!(r.finish(), Err(Error::NoFrame));
    }
}
#[test]
fn chunk_reserved_bytes_abort() {
    for i in [22, 23] {
        let mut r = active();
        let mut p = chunk(7, 1, 0, &[0; 4]);
        p[i] = 1;
        assert_eq!(r.push_chunk(&p), Err(Error::Reserved));
        assert!(!r.is_active());
    }
}
#[test]
fn stale_session_or_frame_aborts() {
    for (s, f, e) in [
        (8, 1, Error::WrongSession),
        (7, 0, Error::WrongFrame),
        (7, 2, Error::WrongFrame),
    ] {
        let mut r = active();
        assert_eq!(r.push_chunk(&chunk(s, f, 0, &[0; 4])), Err(e));
        assert!(!r.is_active());
    }
}
#[test]
fn out_of_order_and_huge_offsets_abort() {
    for offset in [1, 2, u32::MAX] {
        let mut r = active();
        assert_eq!(
            r.push_chunk(&chunk(7, 1, offset, &[0; 2])),
            Err(Error::Offset)
        );
        assert!(!r.is_active());
    }
}
#[test]
fn duplicate_chunk_aborts() {
    let mut r = active();
    let p = chunk(7, 1, 0, &[0; 2]);
    r.push_chunk(&p).unwrap();
    assert_eq!(r.push_chunk(&p), Err(Error::Offset));
    assert!(!r.is_active());
}
#[test]
fn payload_overrun_aborts() {
    let mut r = active();
    assert_eq!(
        r.push_chunk(&chunk(7, 1, 0, &[0; 5])),
        Err(Error::PayloadLength)
    );
    assert!(!r.is_active());
}
#[test]
fn early_finish_discards_partial_frame() {
    let mut r = active();
    r.push_chunk(&chunk(7, 1, 0, &[0; 2])).unwrap();
    assert_eq!(r.finish(), Err(Error::Incomplete));
    assert!(!r.is_active());
    assert_eq!(
        r.begin(&header(7, 1, 2, 1, &[0; 4])),
        Err(Error::StaleFrame)
    );
}
#[test]
fn checksum_failure_discards_complete_frame() {
    let mut r = active();
    r.push_chunk(&chunk(7, 1, 0, &[1; 4])).unwrap();
    assert_eq!(r.finish(), Err(Error::Checksum));
    assert!(!r.is_active());
}
#[test]
fn recover_with_new_id_after_bad_chunk() {
    let mut r = active();
    assert!(r.push_chunk(&[]).is_err());
    r.begin(&header(7, 2, 1, 1, &[0; 2])).unwrap();
    r.push_chunk(&chunk(7, 2, 0, &[0; 2])).unwrap();
    assert!(r.finish().is_ok());
}
#[test]
fn reconnect_requires_new_session_and_rejects_old_packets() {
    let mut r = Reassembler::new(8);
    r.begin(&header(8, 0, 1, 1, &[0; 2])).unwrap();
    assert_eq!(
        r.push_chunk(&chunk(7, 0, 0, &[0; 2])),
        Err(Error::WrongSession)
    );
}
#[test]
fn packets_without_capture_rejected() {
    let mut r = Reassembler::new(7);
    assert_eq!(r.push_chunk(&[]), Err(Error::NoFrame));
    assert_eq!(r.finish(), Err(Error::NoFrame));
}
#[test]
fn literal_header_checks_endian_independently() {
    // width=258, height=3, len=1548; high-bit identities and arbitrary CRC field.
    let b = [
        0x4f, 0x50, 0x43, 0x46, 1, 1, 0, 0, 2, 1, 3, 0, 0x0c, 6, 0, 0, 8, 7, 6, 5, 4, 3, 2, 0x81,
        0x18, 0x17, 0x16, 0x15, 0x14, 0x13, 0x12, 0x91, 0x78, 0x56, 0x34, 0x12,
    ];
    let h = Header::parse(&b).unwrap();
    assert_eq!((h.width, h.height, h.payload_len), (258, 3, 1548));
    assert_eq!(h.session, 0x8102030405060708);
    assert_eq!(h.frame_id, 0x9112131415161718);
    assert_eq!(h.crc32, 0x12345678);
}
#[test]
fn malformed_begin_does_not_consume_id() {
    let mut r = Reassembler::new(7);
    let mut b = header(7, 1, 1, 1, &[0; 2]);
    b[6] = 1;
    assert_eq!(r.begin(&b), Err(Error::Reserved));
    b[6] = 0;
    r.begin(&b).unwrap();
}
#[test]
fn checksum_failure_consumes_id() {
    let mut r = active();
    r.push_chunk(&chunk(7, 1, 0, &[1; 4])).unwrap();
    assert_eq!(r.finish(), Err(Error::Checksum));
    assert_eq!(
        r.begin(&header(7, 1, 2, 1, &[0; 4])),
        Err(Error::StaleFrame)
    );
}
