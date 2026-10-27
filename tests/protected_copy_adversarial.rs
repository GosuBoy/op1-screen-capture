use op1_screen_capture::capture_model::*;
fn ready(m: &mut CaptureModel) {
    let n = m.begin_normalization().unwrap();
    for s in [
        NormalizationStep::OwnersExcluded,
        NormalizationStep::EngineDrained,
        NormalizationStep::StatusCleared,
        NormalizationStep::CoherenceAndLifetimePinned,
    ] {
        m.assume_step(n, s).unwrap();
    }
    m.finish_normalization(n).unwrap();
}
fn plan() -> CopyLayout {
    CopyLayout::new(3, 2, 10, 16).unwrap()
}
#[test]
fn interruption_at_every_window_boundary_keeps_reservation() {
    for prefix in 0..4 {
        for event in 0..4 {
            for close_abort in [false, true] {
                let mut m = CaptureModel::default();
                ready(&mut m);
                let c = m.begin_bounded_capture(plan(), 4).unwrap();
                for _ in 0..prefix {
                    let w = m.begin_read_window(c).unwrap();
                    m.end_read_window(w).unwrap();
                }
                let prior = m.copied_bytes(c).unwrap();
                let w = m.begin_read_window(c).unwrap();
                match event {
                    0 => m.invalidate_lifecycle(),
                    1 => m.dma_fault(),
                    2 => {
                        assert_eq!(
                            m.begin_owner(Owner::Cpu(0)),
                            Err(ModelError::WindowViolation)
                        );
                    }
                    _ => {
                        assert_eq!(
                            m.begin_owner(Owner::Dma(0)),
                            Err(ModelError::WindowViolation)
                        );
                    }
                }
                assert_eq!(m.active_read_window(), Some(w));
                assert_eq!(m.cancel_capture(c), Err(ModelError::ReadWindowOpen));
                assert_eq!(m.finish_copy(c), Err(ModelError::ReadWindowOpen));
                assert_eq!(m.publish(c), Err(ModelError::CopyIncomplete));
                assert!(m.begin_normalization().is_err());
                assert_eq!(m.slot(), Slot::Copying);
                if close_abort {
                    m.abort_read_window(w).unwrap();
                } else {
                    assert_eq!(m.end_read_window(w), Err(ModelError::Invalidated));
                }
                assert_eq!(m.copied_bytes(c), Ok(prior));
                assert_eq!(m.begin_read_window(c), Err(ModelError::Invalidated));
                m.cancel_capture(c).unwrap();
                assert_eq!(m.slot(), Slot::Empty);
                if event < 2 {
                    ready(&mut m);
                    assert!(m.begin_bounded_capture(plan(), 4).is_ok());
                } else {
                    assert_eq!(m.begin_normalization(), Err(ModelError::Poisoned));
                }
            }
        }
    }
}
#[test]
fn every_yield_invalidation_is_sticky_even_after_owner_exit() {
    for prefix in 0..=4 {
        for dma in [false, true] {
            let mut m = CaptureModel::default();
            ready(&mut m);
            let c = m.begin_bounded_capture(plan(), 4).unwrap();
            for _ in 0..prefix {
                let w = m.begin_read_window(c).unwrap();
                m.end_read_window(w).unwrap();
            }
            let before = m.copied_bytes(c).unwrap();
            let lease = m
                .begin_owner(if dma { Owner::Dma(7) } else { Owner::Cpu(7) })
                .unwrap();
            m.end_owner(lease).unwrap();
            assert_eq!(m.begin_read_window(c), Err(ModelError::Invalidated));
            assert_eq!(m.copied_bytes(c), Ok(before));
            assert_eq!(m.publish(c), Err(ModelError::CopyIncomplete));
            assert_eq!(m.finish_copy(c), Err(ModelError::Invalidated));
            assert_eq!(m.slot(), Slot::Empty);
        }
    }
}
#[test]
fn stale_window_events_never_mutate_reused_copy_slot() {
    let mut m = CaptureModel::default();
    ready(&mut m);
    let c = m.begin_bounded_capture(plan(), 4).unwrap();
    let stale = m.begin_read_window(c).unwrap();
    m.abort_read_window(stale).unwrap();
    m.cancel_capture(c).unwrap();
    for _ in 0..256 {
        let current = m.begin_bounded_capture(plan(), 4).unwrap();
        let w = m.begin_read_window(current).unwrap();
        assert_ne!(w, stale);
        assert_eq!(m.end_read_window(stale), Err(ModelError::WrongWindow));
        assert_eq!(m.abort_read_window(stale), Err(ModelError::WrongWindow));
        assert_eq!(m.cancel_capture(c), Err(ModelError::WrongCapture));
        assert_eq!(m.finish_copy(c), Err(ModelError::WrongCapture));
        assert_eq!(m.active_read_window(), Some(w));
        assert_eq!(m.copied_bytes(current), Ok(0));
        m.abort_read_window(w).unwrap();
        m.cancel_capture(current).unwrap();
    }
}
#[test]
fn huge_stride_ranges_are_checked_without_real_memory() {
    for (height, stride, extent, offsets) in [
        (1, usize::MAX - 1, 2, vec![0]),
        (2, usize::MAX - 3, usize::MAX, vec![0, usize::MAX - 3]),
    ] {
        let l = CopyLayout::new(1, height, stride, extent).unwrap();
        let mut m = CaptureModel::default();
        ready(&mut m);
        let c = m.begin_bounded_capture(l, 2).unwrap();
        for (i, offset) in offsets.into_iter().enumerate() {
            let w = m.begin_read_window(c).unwrap();
            assert_eq!(
                (w.source_offset(), w.staging_offset(), w.len()),
                (offset, i * 2, 2)
            );
            assert!(w.source_offset().checked_add(w.len()).unwrap() <= extent);
            m.end_read_window(w).unwrap();
        }
        m.finish_copy(c).unwrap();
        m.publish(c).unwrap();
    }
    assert_eq!(
        CopyLayout::new(1, 2, usize::MAX - 1, usize::MAX),
        Err(ModelError::Layout)
    );
    assert_eq!(
        CopyLayout::new(1, 3, usize::MAX - 3, usize::MAX),
        Err(ModelError::Layout)
    );
}
#[test]
fn bad_new_plan_attempt_does_not_close_current_window() {
    let mut m = CaptureModel::default();
    ready(&mut m);
    let c = m.begin_bounded_capture(plan(), 4).unwrap();
    let w = m.begin_read_window(c).unwrap();
    for limit in [0, 1, 3, 4097, 4098, usize::MAX] {
        assert_eq!(
            m.begin_bounded_capture(plan(), limit),
            Err(ModelError::WindowSize)
        );
        assert_eq!(m.active_read_window(), Some(w));
        assert_eq!(m.copied_bytes(c), Ok(0));
    }
    m.end_read_window(w).unwrap();
    assert_eq!(m.copied_bytes(c), Ok(4));
}
#[test]
fn last_window_still_needs_finish_and_consumer_release() {
    let mut m = CaptureModel::default();
    ready(&mut m);
    let c = m
        .begin_bounded_capture(CopyLayout::new(1, 1, 2, 2).unwrap(), 2)
        .unwrap();
    let w = m.begin_read_window(c).unwrap();
    assert_eq!(m.finish_copy(c), Err(ModelError::ReadWindowOpen));
    m.end_read_window(w).unwrap();
    assert_eq!(m.publish(c), Err(ModelError::CopyIncomplete));
    m.finish_copy(c).unwrap();
    m.publish(c).unwrap();
    m.invalidate_lifecycle();
    assert_eq!(m.published(), Some(c));
    assert_eq!(m.begin_normalization(), Err(ModelError::Busy));
    assert_eq!(m.end_read_window(w), Err(ModelError::WrongWindow));
    m.release_frame(c).unwrap();
    ready(&mut m);
    assert!(m.begin_bounded_capture(plan(), 4).is_ok());
}
