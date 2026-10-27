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
    Layout,
    WindowSize,
    NotBounded,
    ReadWindowOpen,
    WrongWindow,
    CopyComplete,
    WindowViolation,
}

/// Checked model geometry for packed RGB565 output from a potentially padded source.
/// `source_extent` is an asserted accessible extent, not a checked real allocation.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct CopyLayout {
    row_bytes: usize,
    stride: usize,
    source_span: usize,
    packed_bytes: usize,
}

impl CopyLayout {
    pub fn new(
        width: u16,
        height: u16,
        row_stride: usize,
        source_extent: usize,
    ) -> Result<Self, ModelError> {
        if width == 0
            || height == 0
            || width > crate::MAX_DIMENSION
            || height > crate::MAX_DIMENSION
        {
            return Err(ModelError::Layout);
        }
        let row_bytes = usize::from(width) * 2;
        let rows = usize::from(height);
        if row_stride < row_bytes || !row_stride.is_multiple_of(2) {
            return Err(ModelError::Layout);
        }
        let source_span = row_stride
            .checked_mul(rows - 1)
            .and_then(|n| n.checked_add(row_bytes))
            .ok_or(ModelError::Layout)?;
        let packed_bytes = row_bytes.checked_mul(rows).ok_or(ModelError::Layout)?;
        if source_span > source_extent || packed_bytes > crate::MAX_FRAME_BYTES {
            return Err(ModelError::Layout);
        }
        Ok(Self {
            row_bytes,
            stride: row_stride,
            source_span,
            packed_bytes,
        })
    }

    pub fn source_span(self) -> usize {
        self.source_span
    }
    pub fn packed_bytes(self) -> usize {
        self.packed_bytes
    }
}

/// A model-local protected read interval. No source pointer or borrow is retained.
/// Its ranges describe hypothetical reads, not permission to access target memory.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ReadWindow {
    id: u64,
    capture: CaptureId,
    source_offset: usize,
    staging_offset: usize,
    len: usize,
}

impl ReadWindow {
    pub fn source_offset(self) -> usize {
        self.source_offset
    }
    pub fn staging_offset(self) -> usize {
        self.staging_offset
    }
    pub fn len(self) -> usize {
        self.len
    }
    pub fn is_empty(self) -> bool {
        self.len == 0
    }
}

#[derive(Debug, Clone, Copy)]
struct CopyProgress {
    layout: CopyLayout,
    window_limit: usize,
    completed: usize,
}

#[derive(Debug, Clone, Copy)]
struct Candidate {
    id: CaptureId,
    valid: bool,
    copied: bool,
    progress: Option<CopyProgress>,
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
    read_window: Option<ReadWindow>,
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
            read_window: None,
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
    /// Entry during a protected read window permanently poisons this history and
    /// returns no lease: that event violates the asserted exclusion contract.
    pub fn begin_owner(&mut self, owner: Owner) -> Result<OwnerLease, ModelError> {
        if self.read_window.is_some() {
            // This event contradicts the asserted exclusion interval. Do not
            // pretend that dropping a frame repairs an unsafe concurrent read.
            self.poison();
            return Err(ModelError::WindowViolation);
        }
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
            progress: None,
        });
        Ok(id)
    }

    /// Begin a bounded event-only copy. No allocation or real pixel reads occur.
    /// The software window limit is not a proven interrupt-latency budget.
    pub fn begin_bounded_capture(
        &mut self,
        layout: CopyLayout,
        max_window_bytes: usize,
    ) -> Result<CaptureId, ModelError> {
        if max_window_bytes == 0
            || max_window_bytes > crate::MAX_CHUNK_BYTES
            || !max_window_bytes.is_multiple_of(2)
        {
            return Err(ModelError::WindowSize);
        }
        let id = self.begin_capture()?;
        self.candidate
            .as_mut()
            .expect("capture just admitted")
            .progress = Some(CopyProgress {
            layout,
            window_limit: max_window_bytes,
            completed: 0,
        });
        Ok(id)
    }

    pub fn active_read_window(&self) -> Option<ReadWindow> {
        self.read_window
    }

    pub fn copied_bytes(&self, id: CaptureId) -> Result<usize, ModelError> {
        let c = self
            .candidate
            .filter(|c| c.id == id)
            .ok_or(ModelError::WrongCapture)?;
        c.progress
            .map(|p| p.completed)
            .ok_or(ModelError::NotBounded)
    }

    /// Assert that the adapter has established exclusion for this entire read
    /// interval, including autonomous writers, and pinned source geometry/lifetime.
    /// Every actual read must stay within the interval. Do not carry source
    /// references across `end_read_window` or a yield. This method takes no lock.
    pub fn begin_read_window(&mut self, id: CaptureId) -> Result<ReadWindow, ModelError> {
        let c = self
            .candidate
            .filter(|c| c.id == id)
            .ok_or(ModelError::WrongCapture)?;
        if self.read_window.is_some() {
            return Err(ModelError::ReadWindowOpen);
        }
        if !c.valid || self.trust != Trust::Ready {
            return Err(ModelError::Invalidated);
        }
        if self.owner_count() != 0 {
            return Err(ModelError::Busy);
        }
        let p = c.progress.ok_or(ModelError::NotBounded)?;
        if p.completed == p.layout.packed_bytes {
            return Err(ModelError::CopyComplete);
        }
        let row = p.completed / p.layout.row_bytes;
        let column = p.completed % p.layout.row_bytes;
        let len = p.window_limit.min(p.layout.row_bytes - column);
        // Layout construction and bounded progress prove these operations fit.
        let source_offset = row * p.layout.stride + column;
        let window = ReadWindow {
            id: self.next_id()?,
            capture: id,
            source_offset,
            staging_offset: p.completed,
            len,
        };
        self.read_window = Some(window);
        Ok(window)
    }

    /// Confirm the declared reads have finished and all source borrows ended.
    /// Progress is recorded only if the asserted interval remained valid. Even
    /// on invalidation the slot remains reserved until explicit finish/cancel.
    pub fn end_read_window(&mut self, window: ReadWindow) -> Result<(), ModelError> {
        if self.read_window != Some(window) {
            return Err(ModelError::WrongWindow);
        }
        self.read_window = None;
        let c = self
            .candidate
            .as_mut()
            .expect("open window retains candidate");
        if !c.valid || self.trust != Trust::Ready {
            return Err(ModelError::Invalidated);
        }
        let p = c
            .progress
            .as_mut()
            .expect("window only exists for bounded capture");
        p.completed += window.len;
        Ok(())
    }

    /// Confirm a partial/aborted interval has stopped accessing source and staging.
    /// No progress is credited; this permanently invalidates the candidate.
    pub fn abort_read_window(&mut self, window: ReadWindow) -> Result<(), ModelError> {
        if self.read_window != Some(window) {
            return Err(ModelError::WrongWindow);
        }
        self.read_window = None;
        self.invalidate_candidate();
        Ok(())
    }

    /// Record modeled copy completion after all read activity has stopped. Legacy
    /// captures assert completion without a byte count; bounded captures require
    /// every declared byte to have progressed through closed read windows. Neither
    /// path verifies real memory accesses or pixel contents.
    pub fn finish_copy(&mut self, id: CaptureId) -> Result<(), ModelError> {
        let Some(c) = self.candidate.as_mut().filter(|c| c.id == id) else {
            return Err(ModelError::WrongCapture);
        };
        if self.read_window.is_some() {
            return Err(ModelError::ReadWindowOpen);
        }
        if !c.valid {
            self.candidate = None;
            return Err(ModelError::Invalidated);
        }
        if c.copied {
            return Err(ModelError::AlreadyCopied);
        }
        if c.progress
            .is_some_and(|p| p.completed != p.layout.packed_bytes)
        {
            return Err(ModelError::CopyIncomplete);
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
            if self.read_window.is_some() {
                return Err(ModelError::ReadWindowOpen);
            }
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
    #[test]
    fn window_id_exhaustion_retains_candidate_and_never_reuses_id() {
        let mut m = CaptureModel {
            trust: Trust::Ready,
            last_id: u64::MAX - 2,
            ..CaptureModel::default()
        };
        let c = m
            .begin_bounded_capture(CopyLayout::new(2, 1, 4, 4).unwrap(), 2)
            .unwrap();
        let w = m.begin_read_window(c).unwrap();
        assert_eq!(w.id, u64::MAX);
        m.end_read_window(w).unwrap();
        assert_eq!(m.begin_read_window(c), Err(ModelError::IdExhausted));
        assert_eq!(m.trust(), Trust::Poisoned);
        assert_eq!(m.slot(), Slot::Copying);
        assert_eq!(m.active_read_window(), None);
        assert_eq!(m.end_read_window(w), Err(ModelError::WrongWindow));
        assert_eq!(m.copied_bytes(c), Ok(2));
        m.cancel_capture(c).unwrap();
        m.invalidate_lifecycle();
        assert_eq!(m.begin_normalization(), Err(ModelError::Poisoned));
    }
}
