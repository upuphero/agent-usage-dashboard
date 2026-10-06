use crate::domain::*;
use async_trait::async_trait;
use chrono::{DateTime, Utc};
use std::sync::{
    atomic::{AtomicU8, Ordering},
    Arc,
};

/// Cloneable cooperative cancellation. Runners must poll it and reap their child on cancellation.
#[derive(Debug, Clone, Default)]
pub struct CancellationToken(Arc<AtomicU8>);
impl CancellationToken {
    pub fn cancel(&self) {
        let _ = self
            .0
            .compare_exchange(0, 1, Ordering::AcqRel, Ordering::Acquire);
    }
    pub fn is_cancelled(&self) -> bool {
        self.0.load(Ordering::Acquire) == 1
    }
    pub fn check(&self) -> Result<(), CoreError> {
        if self.is_cancelled() {
            Err(CoreError::Cancelled)
        } else {
            Ok(())
        }
    }
    pub fn commit_started(&self) -> bool {
        self.0.load(Ordering::Acquire) == 2
    }
    /// Linearization point: cancellation wins before this transition; publication wins after it.
    pub(crate) fn begin_commit(&self) -> Result<(), CoreError> {
        self.0
            .compare_exchange(0, 2, Ordering::AcqRel, Ordering::Acquire)
            .map(|_| ())
            .map_err(|_| CoreError::Cancelled)
    }
}

pub trait Clock: Send + Sync {
    fn now(&self) -> DateTime<Utc>;
}
/// Opaque native metadata. Scope includes dataset, roots, zone and normalization version.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SourceObservation {
    pub scope: String,
    pub fingerprint: String,
}
/// Dropping this handle stops observation. Events are hints, never parsed usage.
pub trait SourceWatch: Send + Sync {}
pub type ChangeCallback = Arc<dyn Fn() + Send + Sync>;
#[async_trait]
pub trait UsageSource: Send + Sync {
    fn descriptor(&self) -> ProviderDescriptor;
    async fn detect(&self, config: &SourceConfig) -> Result<Detection, CoreError>;
    async fn inspect(
        &self,
        _request: CollectRequest,
        _cancellation: CancellationToken,
    ) -> Result<SourceObservation, CoreError> {
        Err(CoreError::UnsupportedFilter)
    }
    fn watch(
        &self,
        _config: &SourceConfig,
        _changed: ChangeCallback,
    ) -> Result<Box<dyn SourceWatch>, CoreError> {
        Err(CoreError::UnsupportedFilter)
    }
    async fn collect(
        &self,
        request: CollectRequest,
        cancellation: CancellationToken,
    ) -> Result<CollectionBatch, CoreError>;
}
#[async_trait]
pub trait UsageRepository: Send + Sync {
    /// Atomic all-or-nothing replacement. Revisions increment within each SnapshotKey; rows are replaced, never accumulated.
    /// Callers pass validated complete batches. Equal key with another provider/origin is DatasetConflict.
    async fn commit_batch(&self, batch: CollectionBatch) -> Result<CommitResult, CoreError>;
    async fn load_snapshots(
        &self,
        filter: SnapshotFilter,
    ) -> Result<Vec<ReportSnapshot>, CoreError>;
    /// Persist the latest state for this job_id (upsert). No raw process output in records.
    async fn save_scan(&self, scan: ScanRecord) -> Result<(), CoreError>;
    async fn get_scan(&self, job_id: &str) -> Result<Option<ScanRecord>, CoreError>;
    async fn list_scans(&self, provider_id: &str) -> Result<Vec<ScanRecord>, CoreError>;
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn cancellation_and_commit_have_one_winner() {
        let cancelled = CancellationToken::default();
        cancelled.cancel();
        assert_eq!(cancelled.begin_commit(), Err(CoreError::Cancelled));
        assert!(!cancelled.commit_started());
        let committing = CancellationToken::default();
        committing.begin_commit().unwrap();
        committing.cancel();
        assert!(!committing.is_cancelled());
        assert!(committing.commit_started());
    }
}
