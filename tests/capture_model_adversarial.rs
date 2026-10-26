use op1_screen_capture::capture_model::*;
const STEPS: [NormalizationStep; 4] = [
    NormalizationStep::OwnersExcluded,
    NormalizationStep::EngineDrained,
    NormalizationStep::StatusCleared,
    NormalizationStep::CoherenceAndLifetimePinned,
];
fn ready(m: &mut CaptureModel) {
    let n = m.begin_normalization().unwrap();
    for s in STEPS {
        m.assume_step(n, s).unwrap();
    }
    m.finish_normalization(n).unwrap();
}
#[test]
fn consumer_lease_survives_all_external_invalidations() {
    for kind in 0..4 {
        let mut m = CaptureModel::default();
        ready(&mut m);
        let c = m.begin_capture().unwrap();
        m.finish_copy(c).unwrap();
        m.publish(c).unwrap();
        match kind {
            0 => m.invalidate_lifecycle(),
            1 => m.dma_fault(),
            2 => {
                let l = m.begin_owner(Owner::Cpu(0)).unwrap();
                m.end_owner(l).unwrap();
            }
            _ => {
                assert_eq!(
                    m.begin_owner(Owner::Dma(255)),
                    Err(ModelError::UnknownOwner)
                );
            }
        }
        assert_eq!(m.published(), Some(c), "kind {kind}");
        assert_eq!(m.slot(), Slot::Published);
        assert!(m.begin_normalization().is_err());
        assert!(m.begin_capture().is_err());
        m.release_frame(c).unwrap();
        assert_eq!(m.slot(), Slot::Empty);
    }
}
#[test]
fn every_normalization_prefix_can_be_interrupted() {
    for count in 0..=4 {
        for kind in 0..3 {
            let mut m = CaptureModel::default();
            let n = m.begin_normalization().unwrap();
            for s in STEPS.iter().take(count) {
                m.assume_step(n, *s).unwrap();
            }
            match kind {
                0 => m.dma_fault(),
                1 => m.invalidate_lifecycle(),
                _ => {
                    let l = m.begin_owner(Owner::Dma(0)).unwrap();
                    m.end_owner(l).unwrap();
                }
            }
            assert_eq!(
                m.finish_normalization(n),
                Err(ModelError::WrongNormalization)
            );
            assert_eq!(m.begin_capture(), Err(ModelError::Untrusted));
            ready(&mut m);
            assert!(m.begin_capture().is_ok());
        }
    }
}
#[test]
fn old_handles_stay_inert_over_many_lifecycles() {
    let mut m = CaptureModel::default();
    ready(&mut m);
    let old = m.begin_capture().unwrap();
    m.cancel_capture(old).unwrap();
    for _ in 0..1000 {
        m.invalidate_lifecycle();
        ready(&mut m);
        let fresh = m.begin_capture().unwrap();
        assert_ne!(old, fresh);
        assert_eq!(m.finish_copy(old), Err(ModelError::WrongCapture));
        assert_eq!(m.cancel_capture(old), Err(ModelError::WrongCapture));
        assert_eq!(m.publish(old), Err(ModelError::WrongCapture));
        m.finish_copy(fresh).unwrap();
        m.publish(fresh).unwrap();
        assert_eq!(m.release_frame(old), Err(ModelError::NotPublished));
        assert_eq!(m.published(), Some(fresh));
        m.release_frame(fresh).unwrap();
    }
}
#[test]
fn nested_owner_releases_any_order_without_early_admission() {
    for reverse in [false, true] {
        let mut m = CaptureModel::default();
        ready(&mut m);
        let mut ls: Vec<_> = (0..MAX_OWNERS)
            .map(|i| {
                m.begin_owner(if i % 2 == 0 {
                    Owner::Cpu(3)
                } else {
                    Owner::Dma(5)
                })
                .unwrap()
            })
            .collect();
        if reverse {
            ls.reverse();
        }
        for (i, l) in ls.into_iter().enumerate() {
            assert_eq!(m.begin_capture(), Err(ModelError::Busy));
            m.end_owner(l).unwrap();
            assert_eq!(m.owner_count(), MAX_OWNERS - i - 1);
        }
        assert!(m.begin_capture().is_ok());
    }
}
#[test]
fn aborted_attempt_cannot_be_healed_by_events() {
    for stage in 0..2 {
        for kind in 0..3 {
            let mut m = CaptureModel::default();
            ready(&mut m);
            let c = m.begin_capture().unwrap();
            if stage == 1 {
                m.finish_copy(c).unwrap();
            }
            match kind {
                0 => {
                    let l = m.begin_owner(Owner::Cpu(7)).unwrap();
                    m.end_owner(l).unwrap();
                }
                1 => m.invalidate_lifecycle(),
                _ => m.dma_fault(),
            }
            assert_eq!(m.begin_normalization(), Err(ModelError::Busy));
            if stage == 0 {
                assert_eq!(m.publish(c), Err(ModelError::CopyIncomplete));
                assert_eq!(m.slot(), Slot::Copying);
                assert!(m.begin_capture().is_err());
                assert_eq!(m.finish_copy(c), Err(ModelError::Invalidated));
            } else {
                assert_eq!(m.publish(c), Err(ModelError::Invalidated));
            }
            assert_eq!(m.slot(), Slot::Empty);
            if m.trust() != Trust::Ready {
                ready(&mut m);
            }
            let next = m.begin_capture().unwrap();
            assert_ne!(next, c);
            assert_eq!(m.finish_copy(c), Err(ModelError::WrongCapture));
            m.finish_copy(next).unwrap();
            m.publish(next).unwrap();
        }
    }
}
#[test]
fn exhausted_owner_table_never_recovers_on_exit_or_reset() {
    let mut m = CaptureModel::default();
    ready(&mut m);
    let ls: Vec<_> = (0..MAX_OWNERS)
        .map(|_| m.begin_owner(Owner::Cpu(0)).unwrap())
        .collect();
    assert_eq!(m.begin_owner(Owner::Cpu(0)), Err(ModelError::OwnerCapacity));
    for l in ls {
        m.end_owner(l).unwrap();
    }
    m.invalidate_lifecycle();
    m.dma_fault();
    assert_eq!(m.begin_normalization(), Err(ModelError::Poisoned));
    assert_eq!(m.begin_capture(), Err(ModelError::Poisoned));
}
