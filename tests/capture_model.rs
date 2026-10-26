use op1_screen_capture::capture_model::{
    CaptureModel, ModelError as E, NormalizationStep as N, Owner, Slot, Trust, MAX_OWNERS,
};

fn ready(m: &mut CaptureModel) {
    let id = m.begin_normalization().unwrap();
    for step in [
        N::OwnersExcluded,
        N::EngineDrained,
        N::StatusCleared,
        N::CoherenceAndLifetimePinned,
    ] {
        m.assume_step(id, step).unwrap();
    }
    m.finish_normalization(id).unwrap();
}

#[test]
fn default_rejects_capture() {
    let mut m = CaptureModel::default();
    assert_eq!(m.trust(), Trust::Untrusted);
    assert_eq!(m.begin_capture(), Err(E::Untrusted));
}

#[test]
fn normalization_requires_every_ordered_assumption() {
    let mut m = CaptureModel::default();
    let n = m.begin_normalization().unwrap();
    assert_eq!(m.begin_capture(), Err(E::Untrusted));
    assert_eq!(m.finish_normalization(n), Err(E::IncompleteNormalization));
    assert_eq!(m.assume_step(n, N::StatusCleared), Err(E::WrongStep));
    for step in [N::OwnersExcluded, N::EngineDrained, N::StatusCleared] {
        m.assume_step(n, step).unwrap();
        assert_eq!(m.finish_normalization(n), Err(E::IncompleteNormalization));
    }
    m.assume_step(n, N::CoherenceAndLifetimePinned).unwrap();
    m.finish_normalization(n).unwrap();
    assert_eq!(m.trust(), Trust::Ready);
    assert_eq!(m.finish_normalization(n), Err(E::WrongNormalization));
}

#[test]
fn owner_during_normalization_invalidates_cycle() {
    let mut m = CaptureModel::default();
    let n = m.begin_normalization().unwrap();
    let lease = m.begin_owner(Owner::Dma(0)).unwrap();
    m.end_owner(lease).unwrap();
    assert_eq!(m.trust(), Trust::Untrusted);
    assert_eq!(
        m.assume_step(n, N::OwnersExcluded),
        Err(E::WrongNormalization)
    );
    ready(&mut m);
}

#[test]
fn nested_and_suspended_owners_block_capture_and_normalization() {
    let mut m = CaptureModel::default();
    ready(&mut m);
    let a = m.begin_owner(Owner::Cpu(2)).unwrap();
    let b = m.begin_owner(Owner::Cpu(2)).unwrap();
    assert_ne!(a, b);
    m.end_owner(b).unwrap();
    assert_eq!(m.owner_count(), 1);
    assert_eq!(m.begin_capture(), Err(E::Busy));
    assert_eq!(m.begin_normalization(), Err(E::Busy));
    m.invalidate_lifecycle();
    assert_eq!(m.owner_count(), 1);
    assert_eq!(m.begin_normalization(), Err(E::Busy));
    m.end_owner(a).unwrap();
    ready(&mut m);
}

#[test]
fn duplicate_exit_does_not_release_other_lease() {
    let mut m = CaptureModel::default();
    let a = m.begin_owner(Owner::Cpu(0)).unwrap();
    let b = m.begin_owner(Owner::Cpu(0)).unwrap();
    m.end_owner(a).unwrap();
    assert_eq!(m.end_owner(a), Err(E::UnknownLease));
    assert_eq!(m.owner_count(), 1);
    assert_eq!(m.trust(), Trust::Poisoned);
    m.end_owner(b).unwrap();
    assert_eq!(m.begin_normalization(), Err(E::Poisoned));
}

#[test]
fn unknown_owner_and_capacity_overflow_poison() {
    let mut m = CaptureModel::default();
    assert_eq!(m.begin_owner(Owner::Cpu(8)), Err(E::UnknownOwner));
    m.invalidate_lifecycle();
    assert_eq!(m.begin_normalization(), Err(E::Poisoned));
    let mut m = CaptureModel::default();
    let leases: Vec<_> = (0..MAX_OWNERS)
        .map(|_| m.begin_owner(Owner::Dma(7)).unwrap())
        .collect();
    assert_eq!(m.begin_owner(Owner::Cpu(0)), Err(E::OwnerCapacity));
    for lease in leases {
        m.end_owner(lease).unwrap();
    }
    assert_eq!(m.begin_capture(), Err(E::Poisoned));
}

#[test]
fn complete_copy_and_slot_release_are_required() {
    let mut m = CaptureModel::default();
    ready(&mut m);
    let c = m.begin_capture().unwrap();
    assert_eq!(m.publish(c), Err(E::CopyIncomplete));
    assert_eq!(m.begin_normalization(), Err(E::Busy));
    m.finish_copy(c).unwrap();
    assert_eq!(m.finish_copy(c), Err(E::AlreadyCopied));
    m.publish(c).unwrap();
    assert_eq!(m.slot(), Slot::Published);
    assert_eq!(m.begin_capture(), Err(E::Busy));
    assert_eq!(m.begin_normalization(), Err(E::Busy));
    m.release_frame(c).unwrap();
    assert_eq!(m.release_frame(c), Err(E::NotPublished));
    assert_ne!(m.begin_capture().unwrap(), c);
}

#[test]
fn completed_writer_cannot_heal_copy_or_staged_candidate() {
    for staged in [false, true] {
        for owner in [Owner::Cpu(0), Owner::Dma(0)] {
            let mut m = CaptureModel::default();
            ready(&mut m);
            let c = m.begin_capture().unwrap();
            if staged {
                m.finish_copy(c).unwrap();
            }
            let lease = m.begin_owner(owner).unwrap();
            m.end_owner(lease).unwrap();
            assert_eq!(m.trust(), Trust::Ready);
            if staged {
                assert_eq!(m.publish(c), Err(E::Invalidated));
            } else {
                assert_eq!(m.finish_copy(c), Err(E::Invalidated));
            }
            assert_eq!(m.slot(), Slot::Empty);
            assert_eq!(m.publish(c), Err(E::WrongCapture));
        }
    }
}

#[test]
fn publication_is_independent_of_later_normal_writers() {
    let mut m = CaptureModel::default();
    ready(&mut m);
    let c = m.begin_capture().unwrap();
    m.finish_copy(c).unwrap();
    m.publish(c).unwrap();
    let lease = m.begin_owner(Owner::Cpu(0)).unwrap();
    assert_eq!(m.published(), Some(c));
    m.end_owner(lease).unwrap();
    assert_eq!(m.published(), Some(c));
    m.release_frame(c).unwrap();
}

#[test]
fn lifecycle_retains_old_session_slot_until_release() {
    let mut m = CaptureModel::default();
    ready(&mut m);
    let c = m.begin_capture().unwrap();
    m.finish_copy(c).unwrap();
    m.publish(c).unwrap();
    m.invalidate_lifecycle();
    assert_eq!(m.published(), Some(c));
    assert_eq!(m.slot(), Slot::Published);
    assert_eq!(m.begin_capture(), Err(E::Untrusted));
    assert_eq!(m.begin_normalization(), Err(E::Busy));
    m.release_frame(c).unwrap();
    ready(&mut m);
    assert_ne!(m.begin_capture().unwrap(), c);
}

#[test]
fn fault_and_lifecycle_invalidate_staged_copy() {
    for fault in [false, true] {
        let mut m = CaptureModel::default();
        ready(&mut m);
        let c = m.begin_capture().unwrap();
        m.finish_copy(c).unwrap();
        if fault {
            m.dma_fault();
        } else {
            m.invalidate_lifecycle();
        }
        assert_eq!(m.publish(c), Err(E::Invalidated));
        assert_eq!(m.begin_capture(), Err(E::Untrusted));
        ready(&mut m);
        assert_ne!(m.begin_capture().unwrap(), c);
    }
}

#[test]
fn late_dma_exit_is_not_recovery() {
    let mut m = CaptureModel::default();
    ready(&mut m);
    let lease = m.begin_owner(Owner::Dma(0)).unwrap();
    m.dma_fault();
    assert_eq!(m.owner_count(), 1);
    m.end_owner(lease).unwrap();
    assert_eq!(m.begin_capture(), Err(E::Untrusted));
    ready(&mut m);
}

#[test]
fn stale_capture_events_cannot_mutate_replacement() {
    let mut m = CaptureModel::default();
    ready(&mut m);
    let old = m.begin_capture().unwrap();
    m.cancel_capture(old).unwrap();
    let current = m.begin_capture().unwrap();
    assert_eq!(m.finish_copy(old), Err(E::WrongCapture));
    assert_eq!(m.publish(old), Err(E::WrongCapture));
    assert_eq!(m.cancel_capture(old), Err(E::WrongCapture));
    assert_eq!(m.release_frame(old), Err(E::NotPublished));
    assert_eq!(m.slot(), Slot::Copying);
    m.finish_copy(current).unwrap();
    m.publish(current).unwrap();
}

#[test]
fn stale_normalization_cannot_complete_replacement() {
    let mut m = CaptureModel::default();
    let old = m.begin_normalization().unwrap();
    m.dma_fault();
    let current = m.begin_normalization().unwrap();
    assert_eq!(
        m.assume_step(old, N::OwnersExcluded),
        Err(E::WrongNormalization)
    );
    assert_eq!(m.finish_normalization(old), Err(E::WrongNormalization));
    assert_eq!(
        m.finish_normalization(current),
        Err(E::IncompleteNormalization)
    );
}

// Independent compact oracle. Enumerate all 10^5 length-five event histories
// from Ready. Normalization is a macro of four assumptions; partial cycles and
// stale IDs have focused tests above. No claim of unbounded or hardware proof.
#[test]
fn exhaustive_five_event_histories_match_reference() {
    for mut encoded in 0..100_000u32 {
        let mut m = CaptureModel::default();
        ready(&mut m);
        let mut trusted = true;
        let mut candidate: Option<(bool, bool)> = None; // valid, copied
        let mut published = false;
        let mut owners = Vec::new();
        let mut cap = None;
        let mut frame = None;
        for _ in 0..5 {
            let action = encoded % 10;
            encoded /= 10;
            match action {
                0 => {
                    let actual = m.begin_capture();
                    let expected =
                        trusted && owners.is_empty() && candidate.is_none() && !published;
                    assert_eq!(actual.is_ok(), expected);
                    if let Ok(id) = actual {
                        cap = Some(id);
                        candidate = Some((true, false));
                    }
                }
                1 => {
                    if let Some(id) = cap {
                        let expected = match candidate {
                            None => Err(E::WrongCapture),
                            Some((false, _)) => {
                                candidate = None;
                                Err(E::Invalidated)
                            }
                            Some((true, true)) => Err(E::AlreadyCopied),
                            Some((true, false)) => {
                                candidate = Some((true, true));
                                Ok(())
                            }
                        };
                        assert_eq!(m.finish_copy(id), expected);
                    }
                }
                2 => {
                    if let Some(id) = cap {
                        let expected = match candidate {
                            None => Err(E::WrongCapture),
                            Some((_, false)) => Err(E::CopyIncomplete),
                            Some((valid, _)) if !valid || !trusted || !owners.is_empty() => {
                                candidate = None;
                                Err(E::Invalidated)
                            }
                            Some(_) => {
                                candidate = None;
                                published = true;
                                frame = Some(id);
                                Ok(())
                            }
                        };
                        assert_eq!(m.publish(id), expected);
                    }
                }
                3 => {
                    if let Some(id) = cap {
                        let expected = if candidate.take().is_some() {
                            Ok(())
                        } else {
                            Err(E::WrongCapture)
                        };
                        assert_eq!(m.cancel_capture(id), expected);
                    }
                }
                4 => {
                    owners.push(m.begin_owner(Owner::Cpu(0)).unwrap());
                    if let Some(c) = &mut candidate {
                        c.0 = false;
                    }
                }
                5 => {
                    if let Some(lease) = owners.pop() {
                        m.end_owner(lease).unwrap();
                    }
                }
                6 => {
                    m.invalidate_lifecycle();
                    trusted = false;
                    if let Some(c) = &mut candidate {
                        c.0 = false;
                    }
                }
                7 => {
                    m.dma_fault();
                    trusted = false;
                    if let Some(c) = &mut candidate {
                        c.0 = false;
                    }
                }
                8 => {
                    if owners.is_empty() && candidate.is_none() && !published {
                        ready(&mut m);
                        trusted = true;
                    } else {
                        assert_eq!(m.begin_normalization(), Err(E::Busy));
                    }
                }
                9 => {
                    if let Some(id) = frame {
                        let expected = if published {
                            published = false;
                            Ok(())
                        } else {
                            Err(E::NotPublished)
                        };
                        assert_eq!(m.release_frame(id), expected);
                    }
                }
                _ => unreachable!(),
            }
            let expected_slot = match candidate {
                Some((_, true)) => Slot::Staged,
                Some((_, false)) => Slot::Copying,
                None if published => Slot::Published,
                None => Slot::Empty,
            };
            assert_eq!(m.slot(), expected_slot);
            assert_eq!(
                m.trust(),
                if trusted {
                    Trust::Ready
                } else {
                    Trust::Untrusted
                }
            );
            assert_eq!(m.owner_count(), owners.len());
            assert_eq!(m.published().is_some(), published);
        }
    }
}

#[test]
fn invalid_in_progress_copy_retains_slot_until_completion_or_confirmed_cancel() {
    let mut m = CaptureModel::default();
    ready(&mut m);
    let c = m.begin_capture().unwrap();
    m.dma_fault();
    assert_eq!(m.publish(c), Err(E::CopyIncomplete));
    assert_eq!(m.slot(), Slot::Copying);
    assert_eq!(m.begin_normalization(), Err(E::Busy));
    assert_eq!(m.finish_copy(c), Err(E::Invalidated));
    assert_eq!(m.slot(), Slot::Empty);
    ready(&mut m);
}
