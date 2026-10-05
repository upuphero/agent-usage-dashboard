//! In-memory port implementations for deterministic Core tests (no files, processes or wall clock).
use crate::*;
use async_trait::async_trait;
use chrono::{DateTime, Utc};
use std::{collections::BTreeMap, sync::Mutex};

pub struct FixedClock(pub DateTime<Utc>);
impl Clock for FixedClock {
    fn now(&self) -> DateTime<Utc> {
        self.0
    }
}
#[derive(Default)]
pub struct MemoryRepository {
    data: Mutex<MemoryData>,
}
#[derive(Default)]
struct MemoryData {
    snapshots: BTreeMap<SnapshotKey, ReportSnapshot>,
    scans: BTreeMap<String, ScanRecord>,
}
#[async_trait]
impl UsageRepository for MemoryRepository {
    async fn commit_batch(&self, batch: CollectionBatch) -> Result<CommitResult, CoreError> {
        application::validate_batch(&batch)?;
        let mut data = self.data.lock().map_err(|_| CoreError::Storage)?;
        let mut staging = data.snapshots.clone();
        let mut result = CommitResult::default();
        for mut snapshot in batch.snapshots {
            if let Some(old) = staging.get(&snapshot.key) {
                if old.provider_id != snapshot.provider_id
                    || old.origin_device_id != snapshot.origin_device_id
                {
                    return Err(CoreError::DatasetConflict);
                }
                snapshot.revision = old.revision.checked_add(1).ok_or(CoreError::Overflow)?;
            } else {
                snapshot.revision = 1;
            }
            result.snapshots_replaced = result
                .snapshots_replaced
                .checked_add(1)
                .ok_or(CoreError::Overflow)?;
            result.rows_written = result
                .rows_written
                .checked_add(u32::try_from(snapshot.rows.len()).map_err(|_| CoreError::Overflow)?)
                .ok_or(CoreError::Overflow)?;
            staging.insert(snapshot.key.clone(), snapshot);
        }
        data.snapshots = staging;
        Ok(result)
    }
    async fn load_snapshots(
        &self,
        filter: SnapshotFilter,
    ) -> Result<Vec<ReportSnapshot>, CoreError> {
        let data = self.data.lock().map_err(|_| CoreError::Storage)?;
        Ok(data
            .snapshots
            .values()
            .filter(|s| {
                filter
                    .provider_id
                    .as_ref()
                    .is_none_or(|id| id == &s.provider_id)
                    && filter
                        .report_kind
                        .is_none_or(|kind| kind == s.key.report_kind)
                    && filter
                        .timezone
                        .as_ref()
                        .is_none_or(|zone| zone == &s.key.timezone)
            })
            .cloned()
            .collect())
    }
    async fn save_scan(&self, scan: ScanRecord) -> Result<(), CoreError> {
        self.data
            .lock()
            .map_err(|_| CoreError::Storage)?
            .scans
            .insert(scan.job_id.clone(), scan);
        Ok(())
    }
    async fn get_scan(&self, id: &str) -> Result<Option<ScanRecord>, CoreError> {
        Ok(self
            .data
            .lock()
            .map_err(|_| CoreError::Storage)?
            .scans
            .get(id)
            .cloned())
    }
    async fn list_scans(&self, provider: &str) -> Result<Vec<ScanRecord>, CoreError> {
        Ok(self
            .data
            .lock()
            .map_err(|_| CoreError::Storage)?
            .scans
            .values()
            .filter(|s| s.provider_id == provider)
            .cloned()
            .collect())
    }
}
