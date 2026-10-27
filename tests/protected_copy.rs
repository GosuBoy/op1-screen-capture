use op1_screen_capture::capture_model::{
    CaptureModel, CopyLayout, ModelError as E, NormalizationStep as N, Owner, Slot, Trust,
};

fn ready(m: &mut CaptureModel) {
    let n = m.begin_normalization().unwrap();
    for step in [
        N::OwnersExcluded,
        N::EngineDrained,
        N::StatusCleared,
        N::CoherenceAndLifetimePinned,
    ] {
        m.assume_step(n, step).unwrap();
    }
    m.finish_normalization(n).unwrap();
}

fn layout() -> CopyLayout {
    CopyLayout::new(3, 2, 10, 16).unwrap()
}

#[test]
fn invalid_layouts_are_rejected_without_a_capture() {
    for (w, h, stride, extent) in [
        (0, 1, 2, 2),
        (1, 0, 2, 2),
        (513, 1, 1026, 1026),
        (1, 513, 2, 1026),
        (3, 2, 4, 16),
        (3, 2, 7, 20),
        (3, 2, 10, 15),
        (1, 3, usize::MAX - 1, usize::MAX),
    ] {
        assert_eq!(CopyLayout::new(w, h, stride, extent), Err(E::Layout));
    }
    let l = CopyLayout::new(512, 512, 1024, 524_288).unwrap();
    assert_eq!(l.packed_bytes(), 524_288);
    assert_eq!(l.source_span(), 524_288);
}

#[test]
fn invalid_window_limit_never_admits() {
    let mut m = CaptureModel::default();
    ready(&mut m);
    for limit in [0, 1, 3, 4097, 4098, usize::MAX] {
        assert_eq!(m.begin_bounded_capture(layout(), limit), Err(E::WindowSize));
        assert_eq!(m.slot(), Slot::Empty);
    }
    m.begin_bounded_capture(layout(), 4096).unwrap();
}

#[test]
fn bounded_copy_rejects_boot_and_active_owner() {
    let mut m = CaptureModel::default();
    assert_eq!(m.begin_bounded_capture(layout(), 4), Err(E::Untrusted));
    ready(&mut m);
    let owner = m.begin_owner(Owner::Dma(1)).unwrap();
    assert_eq!(m.begin_bounded_capture(layout(), 4), Err(E::Busy));
    m.end_owner(owner).unwrap();
    m.begin_bounded_capture(layout(), 4).unwrap();
}

#[test]
fn bounded_progress_excludes_padding_and_requires_explicit_completion() {
    let mut m = CaptureModel::default();
    ready(&mut m);
    let c = m.begin_bounded_capture(layout(), 4).unwrap();
    for (source, stage, len) in [(0, 0, 4), (4, 4, 2), (10, 6, 4), (14, 10, 2)] {
        assert_eq!(m.finish_copy(c), Err(E::CopyIncomplete));
        assert_eq!(m.publish(c), Err(E::CopyIncomplete));
        let window = m.begin_read_window(c).unwrap();
        assert_eq!(
            (
                window.source_offset(),
                window.staging_offset(),
                window.len()
            ),
            (source, stage, len)
        );
        assert!(!window.is_empty());
        assert_eq!(m.copied_bytes(c).unwrap(), stage);
        m.end_read_window(window).unwrap();
        assert_eq!(m.copied_bytes(c).unwrap(), stage + len);
    }
    assert_eq!(m.begin_read_window(c), Err(E::CopyComplete));
    assert_eq!(m.publish(c), Err(E::CopyIncomplete));
    m.finish_copy(c).unwrap();
    m.publish(c).unwrap();
    assert_eq!(m.slot(), Slot::Published);
}

#[test]
fn open_read_window_blocks_all_early_recycling() {
    let mut m = CaptureModel::default();
    ready(&mut m);
    let c = m.begin_bounded_capture(layout(), 4).unwrap();
    let w = m.begin_read_window(c).unwrap();
    assert_eq!(m.begin_read_window(c), Err(E::ReadWindowOpen));
    assert_eq!(m.finish_copy(c), Err(E::ReadWindowOpen));
    assert_eq!(m.cancel_capture(c), Err(E::ReadWindowOpen));
    assert_eq!(m.publish(c), Err(E::CopyIncomplete));
    assert_eq!(m.begin_normalization(), Err(E::Busy));
    assert_eq!(m.active_read_window(), Some(w));
    assert_eq!(m.slot(), Slot::Copying);
    m.abort_read_window(w).unwrap();
    assert_eq!(m.begin_read_window(c), Err(E::Invalidated));
    m.cancel_capture(c).unwrap();
    assert_eq!(m.slot(), Slot::Empty);
}

#[test]
fn writer_between_windows_permanently_invalidates_attempt() {
    for kind in [Owner::Cpu(0), Owner::Dma(0)] {
        let mut m = CaptureModel::default();
        ready(&mut m);
        let c = m.begin_bounded_capture(layout(), 4).unwrap();
        let w = m.begin_read_window(c).unwrap();
        m.end_read_window(w).unwrap();
        let owner = m.begin_owner(kind).unwrap();
        m.end_owner(owner).unwrap();
        assert_eq!(m.begin_read_window(c), Err(E::Invalidated));
        assert_eq!(m.copied_bytes(c), Ok(4));
        assert_eq!(m.finish_copy(c), Err(E::Invalidated));
        assert_eq!(m.publish(c), Err(E::WrongCapture));
        assert_eq!(m.trust(), Trust::Ready);
    }
}

#[test]
fn writer_inside_window_is_a_permanent_contract_violation() {
    for kind in [Owner::Cpu(0), Owner::Dma(0)] {
        let mut m = CaptureModel::default();
        ready(&mut m);
        let c = m.begin_bounded_capture(layout(), 4).unwrap();
        let w = m.begin_read_window(c).unwrap();
        assert_eq!(m.begin_owner(kind), Err(E::WindowViolation));
        assert_eq!(m.trust(), Trust::Poisoned);
        assert_eq!(m.active_read_window(), Some(w));
        assert_eq!(m.cancel_capture(c), Err(E::ReadWindowOpen));
        assert_eq!(m.end_read_window(w), Err(E::Invalidated));
        assert_eq!(m.copied_bytes(c), Ok(0));
        m.cancel_capture(c).unwrap();
        m.invalidate_lifecycle();
        assert_eq!(m.begin_normalization(), Err(E::Poisoned));
    }
}

#[test]
fn lifecycle_or_fault_cannot_recycle_an_open_window() {
    for fault in [false, true] {
        let mut m = CaptureModel::default();
        ready(&mut m);
        let c = m.begin_bounded_capture(layout(), 4).unwrap();
        let w = m.begin_read_window(c).unwrap();
        if fault {
            m.dma_fault();
        } else {
            m.invalidate_lifecycle();
        }
        assert_eq!(m.cancel_capture(c), Err(E::ReadWindowOpen));
        assert_eq!(m.finish_copy(c), Err(E::ReadWindowOpen));
        assert_eq!(m.active_read_window(), Some(w));
        assert_eq!(m.end_read_window(w), Err(E::Invalidated));
        assert_eq!(m.active_read_window(), None);
        assert_eq!(m.slot(), Slot::Copying);
        assert_eq!(m.copied_bytes(c), Ok(0));
        m.cancel_capture(c).unwrap();
        ready(&mut m);
        let fresh = m.begin_bounded_capture(layout(), 4).unwrap();
        assert_ne!(fresh, c);
        assert_eq!(m.end_read_window(w), Err(E::WrongWindow));
    }
}

#[test]
fn stale_window_cannot_end_or_abort_later_window_or_capture() {
    let mut m = CaptureModel::default();
    ready(&mut m);
    let c = m.begin_bounded_capture(layout(), 4).unwrap();
    let old = m.begin_read_window(c).unwrap();
    m.end_read_window(old).unwrap();
    let current = m.begin_read_window(c).unwrap();
    assert_eq!(m.end_read_window(old), Err(E::WrongWindow));
    assert_eq!(m.abort_read_window(old), Err(E::WrongWindow));
    assert_eq!(m.active_read_window(), Some(current));
    assert_eq!(m.copied_bytes(c), Ok(4));
    m.abort_read_window(current).unwrap();
    m.cancel_capture(c).unwrap();
    let fresh = m.begin_bounded_capture(layout(), 4).unwrap();
    let w = m.begin_read_window(fresh).unwrap();
    assert_eq!(m.end_read_window(current), Err(E::WrongWindow));
    assert_eq!(m.abort_read_window(current), Err(E::WrongWindow));
    assert_eq!(m.active_read_window(), Some(w));
    m.end_read_window(w).unwrap();
}

#[test]
fn last_chunk_does_not_bypass_post_copy_invalidation() {
    let mut m = CaptureModel::default();
    ready(&mut m);
    let c = m
        .begin_bounded_capture(CopyLayout::new(1, 1, 2, 2).unwrap(), 2)
        .unwrap();
    let w = m.begin_read_window(c).unwrap();
    m.end_read_window(w).unwrap();
    m.finish_copy(c).unwrap();
    let owner = m.begin_owner(Owner::Cpu(0)).unwrap();
    m.end_owner(owner).unwrap();
    assert_eq!(m.publish(c), Err(E::Invalidated));
}

#[test]
fn legacy_capture_has_no_implicit_window_progress() {
    let mut m = CaptureModel::default();
    ready(&mut m);
    let c = m.begin_capture().unwrap();
    assert_eq!(m.begin_read_window(c), Err(E::NotBounded));
    assert_eq!(m.copied_bytes(c), Err(E::NotBounded));
    m.finish_copy(c).unwrap();
    m.publish(c).unwrap();
}

// Exhaust the small geometry domain independently with a row-by-row range oracle.
// 8 widths * 5 heights * 3 paddings * 9 window limits = 1080 layouts.
#[test]
fn exhaustive_small_layouts_cover_exact_active_pixels_once() {
    for width in 1..=8u16 {
        for height in 1..=5u16 {
            for padding in [0, 2, 4] {
                for limit in (2..=18).step_by(2) {
                    let row = usize::from(width) * 2;
                    let stride = row + padding;
                    let extent = (usize::from(height) - 1) * stride + row;
                    let l = CopyLayout::new(width, height, stride, extent).unwrap();
                    let expected: Vec<_> = (0..usize::from(height))
                        .flat_map(|y| {
                            (0..row)
                                .step_by(limit)
                                .map(move |x| (y * stride + x, y * row + x, limit.min(row - x)))
                        })
                        .collect();
                    let mut m = CaptureModel::default();
                    ready(&mut m);
                    let c = m.begin_bounded_capture(l, limit).unwrap();
                    for (src, dst, len) in expected {
                        let w = m.begin_read_window(c).unwrap();
                        assert_eq!(
                            (w.source_offset(), w.staging_offset(), w.len()),
                            (src, dst, len)
                        );
                        assert!(src + len <= extent);
                        assert!(dst + len <= l.packed_bytes());
                        m.end_read_window(w).unwrap();
                    }
                    assert_eq!(m.copied_bytes(c), Ok(l.packed_bytes()));
                    assert_eq!(m.begin_read_window(c), Err(E::CopyComplete));
                    m.finish_copy(c).unwrap();
                    m.publish(c).unwrap();
                }
            }
        }
    }
}
