//! Serialized, host-only capture contract model. Not a firmware lock or DMA driver.
//!
//! Every event is supplied by a hypothetical observer. Normalization steps are
//! assumptions, not hardware observations. Missing events invalidate the argument.
//! IDs are local to one model instance and must never be replayed into another.
//! No memory is copied; publication means only acceptance of modeled metadata.

/// Maximum simultaneously modeled owner leases, including suspended owners.
pub const MAX_OWNERS: usize = 16;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Owner {
    Cpu(u8),
    Dma(u8),
}

impl Owner {
    fn valid(self) -> bool {
        match self {
            Self::Cpu(i) | Self::Dma(i) => i < 8,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct OwnerLease(u64);
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct CaptureId(u64);
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct NormalizationId(u64);

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Trust {
    Untrusted,
    Normalizing,
    Ready,
    /// An unrepresentable or inconsistent owner history cannot be recovered here.
    Poisoned,
}

/// Ordered assumptions about a hypothetical target, not proof of those facts.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum NormalizationStep {
    OwnersExcluded,
    EngineDrained,
    StatusCleared,
    CoherenceAndLifetimePinned,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Slot {
    Empty,
    Copying,
    Staged,
    Published,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ModelError {
    Untrusted,
    Poisoned,
    Busy,
    WrongNormalization,
    WrongStep,
    IncompleteNormalization,
    UnknownOwner,
    OwnerCapacity,
    UnknownLease,
    WrongCapture,
    CopyIncomplete,
    AlreadyCopied,
    Invalidated,
    NotPublished,
    IdExhausted,
}

#[derive(Debug, Clone, Copy)]
struct Candidate {
    id: CaptureId,
    valid: bool,
    copied: bool,
}

/// One serialized event history. Not Clone: a snapshot is not a new ID namespace.
#[derive(Debug)]
pub struct CaptureModel {
    trust: Trust,
    normalization: Option<(NormalizationId, u8)>,
    owners: [Option<(OwnerLease, Owner)>; MAX_OWNERS],
    candidate: Option<Candidate>,
    published: Option<CaptureId>,
    last_id: u64,
}

impl Default for CaptureModel {
    fn default() -> Self {
        Self {
            trust: Trust::Untrusted,
            normalization: None,
            owners: [None; MAX_OWNERS],
            candidate: None,
            published: None,
            last_id: 0,
        }
    }
}

impl CaptureModel {
    pub fn trust(&self) -> Trust {
        self.trust
    }

    pub fn owner_count(&self) -> usize {
        self.owners.iter().flatten().count()
    }

    pub fn slot(&self) -> Slot {
        match self.candidate {
            Some(c) if c.copied => Slot::Staged,
            Some(_) => Slot::Copying,
            None if self.published.is_some() => Slot::Published,
            None => Slot::Empty,
        }
    }

    /// Identifies the reserved published slot, not permission to send it in the
    /// current session. Lifecycle changes do not recycle consumer-owned storage.
    pub fn published(&self) -> Option<CaptureId> {
        self.published
    }

    fn invalidate_candidate(&mut self) {
        if let Some(c) = &mut self.candidate {
            c.valid = false;
        }
    }

    fn lose_trust(&mut self) {
        if self.trust != Trust::Poisoned {
            self.trust = Trust::Untrusted;
        }
        self.normalization = None;
        self.invalidate_candidate();
    }

    fn poison(&mut self) {
        self.lose_trust();
        self.trust = Trust::Poisoned;
    }

    fn next_id(&mut self) -> Result<u64, ModelError> {
        match self.last_id.checked_add(1) {
            Some(id) => {
                self.last_id = id;
                Ok(id)
            }
            None => {
                self.poison();
                Err(ModelError::IdExhausted)
            }
        }
    }

    /// Record entry before the owner's first write. A suspended owner retains its
    /// lease until it has actually exited. Nested entries each need their own lease.
    pub fn begin_owner(&mut self, owner: Owner) -> Result<OwnerLease, ModelError> {
        self.invalidate_candidate();
        if self.trust == Trust::Normalizing {
            self.lose_trust();
        }
        if self.trust == Trust::Poisoned {
            return Err(ModelError::Poisoned);
        }
        if !owner.valid() {
            self.poison();
            return Err(ModelError::UnknownOwner);
        }
        let Some(index) = self.owners.iter().position(Option::is_none) else {
            self.poison();
            return Err(ModelError::OwnerCapacity);
        };
        let lease = OwnerLease(self.next_id()?);
        self.owners[index] = Some((lease, owner));
        Ok(lease)
    }

    /// A stale or duplicate exit never releases a different owner's lease.
    /// Inconsistent owner bookkeeping poisons admission rather than guessing.
    pub fn end_owner(&mut self, lease: OwnerLease) -> Result<(), ModelError> {
        if let Some(index) = self
            .owners
            .iter()
            .position(|entry| entry.is_some_and(|(id, _)| id == lease))
        {
            self.owners[index] = None;
            Ok(())
        } else {
            self.poison();
            Err(ModelError::UnknownLease)
        }
    }

    pub fn begin_normalization(&mut self) -> Result<NormalizationId, ModelError> {
        if self.trust == Trust::Poisoned {
            return Err(ModelError::Poisoned);
        }
        if self.owner_count() != 0 || self.slot() != Slot::Empty || self.trust == Trust::Normalizing
        {
            return Err(ModelError::Busy);
        }
        let id = NormalizationId(self.next_id()?);
        self.trust = Trust::Normalizing;
        self.normalization = Some((id, 0));
        Ok(id)
    }

    /// Assert an ordered prerequisite in this model. This does not read registers,
    /// exclude real owners, drain hardware, or establish safe memory access.
    pub fn assume_step(
        &mut self,
        id: NormalizationId,
        step: NormalizationStep,
    ) -> Result<(), ModelError> {
        let Some((current, index)) = self.normalization else {
            return Err(ModelError::WrongNormalization);
        };
        if current != id {
            return Err(ModelError::WrongNormalization);
        }
        let expected = [
            NormalizationStep::OwnersExcluded,
            NormalizationStep::EngineDrained,
            NormalizationStep::StatusCleared,
            NormalizationStep::CoherenceAndLifetimePinned,
        ];
        if expected.get(usize::from(index)) != Some(&step) {
            return Err(ModelError::WrongStep);
        }
        self.normalization = Some((id, index + 1));
        Ok(())
    }

    pub fn finish_normalization(&mut self, id: NormalizationId) -> Result<(), ModelError> {
        match self.normalization {
            Some((current, 4)) if current == id => {
                self.normalization = None;
                self.trust = Trust::Ready;
                Ok(())
            }
            Some((current, _)) if current == id => Err(ModelError::IncompleteNormalization),
            _ => Err(ModelError::WrongNormalization),
        }
    }

    /// Invalidates lifecycle/geometry/lifetime assumptions. Existing owner leases
    /// remain active. Published storage stays reserved until its consumer confirms
    /// completion or cancellation through `release_frame`, including after reset.
    pub fn invalidate_lifecycle(&mut self) {
        self.lose_trust();
    }

    /// Includes timeout, uncertain completion, or DMA error. An eventual owner exit
    /// does not restore trust; a new complete normalization cycle is required.
    pub fn dma_fault(&mut self) {
        self.lose_trust();
    }

    pub fn begin_capture(&mut self) -> Result<CaptureId, ModelError> {
        if self.trust == Trust::Poisoned {
            return Err(ModelError::Poisoned);
        }
        if self.trust != Trust::Ready {
            return Err(ModelError::Untrusted);
        }
        if self.owner_count() != 0 || self.slot() != Slot::Empty {
            return Err(ModelError::Busy);
        }
        let id = CaptureId(self.next_id()?);
        self.candidate = Some(Candidate {
            id,
            valid: true,
            copied: false,
        });
        Ok(id)
    }

    /// Record a full modeled copy into independently owned staging. Partial copies
    /// must be cancelled instead. No real byte count or memory access is checked.
    pub fn finish_copy(&mut self, id: CaptureId) -> Result<(), ModelError> {
        let Some(c) = self.candidate.as_mut().filter(|c| c.id == id) else {
            return Err(ModelError::WrongCapture);
        };
        if !c.valid {
            self.candidate = None;
            return Err(ModelError::Invalidated);
        }
        if c.copied {
            return Err(ModelError::AlreadyCopied);
        }
        c.copied = true;
        Ok(())
    }

    /// Accept only a complete, never-invalidated candidate. A rejected invalid
    /// candidate is consumed, while a stale ID cannot affect a current candidate.
    pub fn publish(&mut self, id: CaptureId) -> Result<(), ModelError> {
        let Some(c) = self.candidate.filter(|c| c.id == id) else {
            return Err(ModelError::WrongCapture);
        };
        if !c.copied {
            return Err(ModelError::CopyIncomplete);
        }
        if !c.valid || self.trust != Trust::Ready || self.owner_count() != 0 {
            self.candidate = None;
            return Err(ModelError::Invalidated);
        }
        self.candidate = None;
        self.published = Some(id);
        Ok(())
    }

    /// Record confirmed cancellation: copying has stopped and no remaining copy
    /// activity can access staging. This is not an asynchronous cancellation request.
    pub fn cancel_capture(&mut self, id: CaptureId) -> Result<(), ModelError> {
        if self.candidate.is_some_and(|c| c.id == id) {
            self.candidate = None;
            Ok(())
        } else {
            Err(ModelError::WrongCapture)
        }
    }

    /// Release the sole modeled owned staging slot after its consumer is finished.
    pub fn release_frame(&mut self, id: CaptureId) -> Result<(), ModelError> {
        if self.published == Some(id) {
            self.published = None;
            Ok(())
        } else {
            Err(ModelError::NotPublished)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn id_exhaustion_poisoned_without_reuse() {
        let mut m = CaptureModel {
            last_id: u64::MAX - 1,
            ..CaptureModel::default()
        };
        let last = m.begin_owner(Owner::Cpu(0)).unwrap();
        assert_eq!(last.0, u64::MAX);
        m.end_owner(last).unwrap();
        assert_eq!(m.begin_normalization(), Err(ModelError::IdExhausted));
        assert_eq!(m.trust(), Trust::Poisoned);
        m.invalidate_lifecycle();
        assert_eq!(m.begin_capture(), Err(ModelError::Poisoned));
    }
}
