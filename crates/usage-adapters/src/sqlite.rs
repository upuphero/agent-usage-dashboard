use async_trait::async_trait;
use chrono::{DateTime, Utc};
use rusqlite::{params, Connection, OptionalExtension, TransactionBehavior, MAIN_DB};
use rust_decimal::Decimal;
use sha2::{Digest, Sha256};
use std::{
    path::Path,
    str::FromStr,
    sync::{Arc, Mutex},
    time::Duration,
};
use usage_core::*;

const VERSION: i64 = 1;
const MIGRATION: &str = include_str!("../migrations/001_initial.sql");
fn migration_checksum() -> String {
    // Source checkouts on Windows can use CRLF; schema identity must survive that conversion.
    format!(
        "{:x}",
        Sha256::digest(MIGRATION.replace("\r\n", "\n").as_bytes())
    )
}

/// One serialized connection shared by clones. SQLite's write transaction also coordinates
/// independently opened repositories; blocking SQL never holds up the async runtime worker.
#[derive(Clone)]
pub struct SqliteRepository {
    connection: Arc<Mutex<Connection>>,
}
impl SqliteRepository {
    /// Host chooses its application-data path. A newer schema is rejected before any pragma writes.
    pub fn open(path: impl AsRef<Path>) -> Result<Self, CoreError> {
        let path = path.as_ref();
        let mut connection = Connection::open(path).map_err(storage)?;
        initialize(&mut connection, Some(path))?;
        Ok(Self {
            connection: Arc::new(Mutex::new(connection)),
        })
    }
    pub fn in_memory() -> Result<Self, CoreError> {
        let mut connection = Connection::open_in_memory().map_err(storage)?;
        initialize(&mut connection, None)?;
        Ok(Self {
            connection: Arc::new(Mutex::new(connection)),
        })
    }
    /// Consistent SQLite backup including committed WAL content. Refuses to overwrite a file.
    pub fn backup_to(&self, destination: impl AsRef<Path>) -> Result<(), CoreError> {
        let destination = destination.as_ref();
        let reserved = std::fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(destination)
            .map_err(|_| CoreError::Storage)?;
        drop(reserved);
        let connection = self.connection.lock().map_err(|_| CoreError::Storage)?;
        let result = connection
            .backup(MAIN_DB, destination, None)
            .map_err(storage);
        if result.is_err() {
            let _ = std::fs::remove_file(destination);
        }
        result
    }
    async fn with_connection<T: Send + 'static>(
        &self,
        operation: impl FnOnce(&mut Connection) -> Result<T, CoreError> + Send + 'static,
    ) -> Result<T, CoreError> {
        let connection = self.connection.clone();
        tokio::task::spawn_blocking(move || {
            let mut connection = connection.lock().map_err(|_| CoreError::Storage)?;
            operation(&mut connection)
        })
        .await
        .map_err(|_| CoreError::Storage)?
    }
}

fn storage(_: rusqlite::Error) -> CoreError {
    CoreError::Storage
}
fn json<T: serde::Serialize>(value: &T) -> Result<String, CoreError> {
    serde_json::to_string(value).map_err(|_| CoreError::Storage)
}
fn parse<T: serde::de::DeserializeOwned>(text: &str) -> Result<T, CoreError> {
    serde_json::from_str(text).map_err(|_| CoreError::Storage)
}
fn kind(kind: ReportKind) -> &'static str {
    match kind {
        ReportKind::Daily => "daily",
        ReportKind::Session => "session",
    }
}
fn accuracy(value: Accuracy) -> &'static str {
    match value {
        Accuracy::Exact => "exact",
        Accuracy::Derived => "derived",
        Accuracy::Estimated => "estimated",
        Accuracy::Unavailable => "unavailable",
    }
}
fn parse_accuracy(value: &str) -> Result<Accuracy, CoreError> {
    match value {
        "exact" => Ok(Accuracy::Exact),
        "derived" => Ok(Accuracy::Derived),
        "estimated" => Ok(Accuracy::Estimated),
        "unavailable" => Ok(Accuracy::Unavailable),
        _ => Err(CoreError::Storage),
    }
}
fn metric(row: &rusqlite::Row<'_>, value: usize) -> Result<Metric<u64>, CoreError> {
    let amount: Option<i64> = row.get(value).map_err(storage)?;
    let result = Metric {
        value: amount
            .map(|v| u64::try_from(v).map_err(|_| CoreError::Storage))
            .transpose()?,
        accuracy: parse_accuracy(&row.get::<_, String>(value + 1).map_err(storage)?)?,
    };
    result.validate().map_err(|_| CoreError::Storage)?;
    Ok(result)
}
fn time(value: Option<String>) -> Result<Option<DateTime<Utc>>, CoreError> {
    value
        .map(|v| {
            DateTime::parse_from_rfc3339(&v)
                .map(|v| v.with_timezone(&Utc))
                .map_err(|_| CoreError::Storage)
        })
        .transpose()
}

fn initialize(connection: &mut Connection, path: Option<&Path>) -> Result<(), CoreError> {
    let version: i64 = connection
        .pragma_query_value(None, "user_version", |r| r.get(0))
        .map_err(storage)?;
    if version > VERSION {
        return Err(CoreError::StorageSchemaNewer);
    }
    connection
        .busy_timeout(Duration::from_secs(2))
        .map_err(storage)?;
    connection
        .pragma_update(None, "foreign_keys", "ON")
        .map_err(storage)?;
    if version < VERSION {
        let table_count: i64 = connection.query_row("SELECT count(*) FROM sqlite_master WHERE type='table' AND name NOT LIKE 'sqlite_%'", [], |r| r.get(0)).map_err(storage)?;
        if table_count > 0 {
            if let Some(path) = path {
                let backup = path.with_extension(format!(
                    "pre-migration-v{version}-{}.sqlite3",
                    uuid::Uuid::new_v4()
                ));
                connection.backup(MAIN_DB, &backup, None).map_err(storage)?;
            }
        }
        let transaction = connection
            .transaction_with_behavior(TransactionBehavior::Immediate)
            .map_err(storage)?;
        transaction.execute_batch(MIGRATION).map_err(storage)?;
        transaction
            .execute(
                "INSERT INTO schema_migrations(version,checksum) VALUES(?1,?2)",
                params![VERSION, migration_checksum()],
            )
            .map_err(storage)?;
        transaction
            .pragma_update(None, "user_version", VERSION)
            .map_err(storage)?;
        transaction.commit().map_err(storage)?;
    }
    let checksum: String = connection
        .query_row(
            "SELECT checksum FROM schema_migrations WHERE version=?1",
            [VERSION],
            |r| r.get(0),
        )
        .map_err(storage)?;
    let max: i64 = connection
        .query_row("SELECT max(version) FROM schema_migrations", [], |r| {
            r.get(0)
        })
        .map_err(storage)?;
    if max > VERSION {
        return Err(CoreError::StorageSchemaNewer);
    }
    if checksum != migration_checksum() {
        return Err(CoreError::Storage);
    }
    connection
        .pragma_update(None, "journal_mode", "WAL")
        .map_err(storage)?;
    connection
        .pragma_update(None, "synchronous", "NORMAL")
        .map_err(storage)?;
    Ok(())
}

#[async_trait]
impl UsageRepository for SqliteRepository {
    async fn commit_batch(&self, batch: CollectionBatch) -> Result<CommitResult, CoreError> {
        usage_core::application::validate_batch(&batch)?;
        self.with_connection(move |connection| {
            let transaction = connection.transaction_with_behavior(TransactionBehavior::Immediate).map_err(storage)?;
            let mut result = CommitResult::default();
            for mut snapshot in batch.snapshots {
                let key = json(&snapshot.key)?;
                transaction.execute("INSERT INTO devices(device_id) VALUES(?1) ON CONFLICT DO NOTHING", [&snapshot.origin_device_id]).map_err(storage)?;
                transaction.execute("INSERT INTO source_datasets(dataset_id,provider_id,product_id,origin_device_id) VALUES(?1,?2,?3,?4) ON CONFLICT DO NOTHING", params![snapshot.key.source_dataset_id,snapshot.provider_id,snapshot.key.product_id,snapshot.origin_device_id]).map_err(storage)?;
                let owner: (String,String,String) = transaction.query_row("SELECT provider_id,product_id,origin_device_id FROM source_datasets WHERE dataset_id=?1", [&snapshot.key.source_dataset_id], |r| Ok((r.get(0)?,r.get(1)?,r.get(2)?))).map_err(storage)?;
                if owner != (snapshot.provider_id.clone(),snapshot.key.product_id.clone(),snapshot.origin_device_id.clone()) { return Err(CoreError::DatasetConflict); }
                let revision: Option<i64> = transaction.query_row("SELECT revision FROM report_snapshots WHERE snapshot_key=?1", [&key], |r| r.get(0)).optional().map_err(storage)?;
                let revision = revision.unwrap_or(0).checked_add(1).ok_or(CoreError::Overflow)?;
                snapshot.revision = u64::try_from(revision).map_err(|_| CoreError::Storage)?;
                let rows = std::mem::take(&mut snapshot.rows);
                // All mutations remain private to this transaction until every snapshot has succeeded.
                transaction.execute("INSERT INTO report_snapshots(snapshot_key,dataset_id,provider_id,product_id,report_kind,timezone,scope_key,revision,metadata_json) VALUES(?1,?2,?3,?4,?5,?6,?7,?8,?9) ON CONFLICT(snapshot_key) DO UPDATE SET revision=excluded.revision,metadata_json=excluded.metadata_json", params![key,snapshot.key.source_dataset_id,snapshot.provider_id,snapshot.key.product_id,kind(snapshot.key.report_kind),snapshot.key.timezone,json(&snapshot.key.scope)?,revision,json(&snapshot)?]).map_err(storage)?;
                transaction.execute("DELETE FROM report_rows WHERE snapshot_key=?1", [&key]).map_err(storage)?;
                let mut insert = transaction.prepare_cached("INSERT INTO report_rows(snapshot_key,row_key,dimension_kind,local_date,session_id,model_key,model_id,model_vendor,input_uncached,input_accuracy,cache_read,cache_read_accuracy,cache_write,cache_write_accuracy,output_total,output_accuracy,output_reasoning,reasoning_accuracy,total,total_accuracy,amount_usd,cost_accuracy,pricing_version,pricing_as_of,missing_models_json,session_started_at,last_activity_at) VALUES(?1,?2,?3,?4,?5,?6,?7,?8,?9,?10,?11,?12,?13,?14,?15,?16,?17,?18,?19,?20,?21,?22,?23,?24,?25,?26,?27)").map_err(storage)?;
                for row in &rows {
                    let (dimension,date,session) = match &row.key.dimension { RowDimension::Day(day) => ("day",Some(day.to_string()),None), RowDimension::Session(id) => ("session",None,Some(id.clone())) };
                    insert.execute(params![key,json(&row.key)?,dimension,date,session,json(&row.key.model_id)?,row.key.model_id,row.model_vendor,
                        row.tokens.input_uncached.value.map(|v|v as i64),accuracy(row.tokens.input_uncached.accuracy),row.tokens.cache_read.value.map(|v|v as i64),accuracy(row.tokens.cache_read.accuracy),row.tokens.cache_write.value.map(|v|v as i64),accuracy(row.tokens.cache_write.accuracy),row.tokens.output_total.value.map(|v|v as i64),accuracy(row.tokens.output_total.accuracy),row.tokens.output_reasoning.value.map(|v|v as i64),accuracy(row.tokens.output_reasoning.accuracy),row.tokens.total.value.map(|v|v as i64),accuracy(row.tokens.total.accuracy),row.cost.amount_usd.value.map(|v|v.to_string()),accuracy(row.cost.amount_usd.accuracy),row.cost.pricing_version,row.cost.pricing_as_of.map(|v|v.to_rfc3339()),json(&row.cost.missing_models)?,row.session_started_at.map(|v|v.to_rfc3339()),row.last_activity_at.map(|v|v.to_rfc3339())]).map_err(storage)?;
                }
                result.snapshots_replaced = result.snapshots_replaced.checked_add(1).ok_or(CoreError::Overflow)?;
                result.rows_written = result.rows_written.checked_add(u32::try_from(rows.len()).map_err(|_|CoreError::Overflow)?).ok_or(CoreError::Overflow)?;
            }
            transaction.commit().map_err(storage)?;
            Ok(result)
        }).await
    }
    async fn load_snapshots(
        &self,
        filter: SnapshotFilter,
    ) -> Result<Vec<ReportSnapshot>, CoreError> {
        self.with_connection(move |connection| {
            // A read transaction binds metadata and rows to the same committed revision, even
            // while another repository connection publishes a newer batch.
            let transaction = connection.transaction().map_err(storage)?;
            let mut query = transaction.prepare("SELECT snapshot_key,revision,metadata_json FROM report_snapshots WHERE (?1 IS NULL OR provider_id=?1) AND (?2 IS NULL OR report_kind=?2) AND (?3 IS NULL OR timezone=?3) ORDER BY snapshot_key").map_err(storage)?;
            let mut metadata = query.query(params![filter.provider_id,filter.report_kind.map(kind),filter.timezone]).map_err(storage)?;
            let mut result = Vec::new();
            while let Some(row) = metadata.next().map_err(storage)? {
                let key: String = row.get(0).map_err(storage)?;
                let revision: i64 = row.get(1).map_err(storage)?;
                let mut snapshot: ReportSnapshot = parse(&row.get::<_,String>(2).map_err(storage)?)?;
                if json(&snapshot.key)? != key || snapshot.revision != revision as u64 || !snapshot.rows.is_empty() { return Err(CoreError::Storage); }
                let mut statement = transaction.prepare("SELECT row_key,model_vendor,input_uncached,input_accuracy,cache_read,cache_read_accuracy,cache_write,cache_write_accuracy,output_total,output_accuracy,output_reasoning,reasoning_accuracy,total,total_accuracy,amount_usd,cost_accuracy,pricing_version,pricing_as_of,missing_models_json,session_started_at,last_activity_at FROM report_rows WHERE snapshot_key=?1 ORDER BY row_key").map_err(storage)?;
                let mut rows = statement.query([&key]).map_err(storage)?;
                while let Some(row) = rows.next().map_err(storage)? {
                    let amount: Option<String> = row.get(14).map_err(storage)?;
                    snapshot.rows.push(ReportRow { key: parse(&row.get::<_,String>(0).map_err(storage)?)?, model_vendor: row.get(1).map_err(storage)?, tokens: TokenMetrics { input_uncached: metric(row,2)?,cache_read:metric(row,4)?,cache_write:metric(row,6)?,output_total:metric(row,8)?,output_reasoning:metric(row,10)?,total:metric(row,12)? }, cost: CostEstimate { amount_usd: Metric { value: amount.map(|v|Decimal::from_str(&v).map_err(|_|CoreError::Storage)).transpose()?,accuracy:parse_accuracy(&row.get::<_,String>(15).map_err(storage)?)? },pricing_version:row.get(16).map_err(storage)?,pricing_as_of:time(row.get(17).map_err(storage)?)?,missing_models:parse(&row.get::<_,String>(18).map_err(storage)?)? }, session_started_at:time(row.get(19).map_err(storage)?)?,last_activity_at:time(row.get(20).map_err(storage)?)? });
                }
                snapshot.rows.sort_by(|a,b|a.key.cmp(&b.key));
                result.push(snapshot);
            }
            result.sort_by(|a,b|a.key.cmp(&b.key));
            if !result.is_empty() { usage_core::application::validate_batch(&CollectionBatch { snapshots:result.clone() }).map_err(|_|CoreError::Storage)?; }
            Ok(result)
        }).await
    }
    async fn save_scan(&self, scan: ScanRecord) -> Result<(), CoreError> {
        self.with_connection(move |connection| {
            let count = connection.execute("INSERT INTO scan_runs(job_id,provider_id,started_at,record_json) VALUES(?1,?2,?3,?4) ON CONFLICT(job_id) DO UPDATE SET started_at=excluded.started_at,record_json=excluded.record_json WHERE scan_runs.provider_id=excluded.provider_id",params![scan.job_id,scan.provider_id,scan.started_at.to_rfc3339(),json(&scan)?]).map_err(storage)?;
            if count != 1 { return Err(CoreError::DatasetConflict); }
            Ok(())
        }).await
    }
    async fn get_scan(&self, job_id: &str) -> Result<Option<ScanRecord>, CoreError> {
        let job_id = job_id.to_owned();
        self.with_connection(move |connection| {
            let text: Option<String> = connection
                .query_row(
                    "SELECT record_json FROM scan_runs WHERE job_id=?1",
                    [job_id],
                    |r| r.get(0),
                )
                .optional()
                .map_err(storage)?;
            text.map(|text| parse(&text)).transpose()
        })
        .await
    }
    async fn list_scans(&self, provider_id: &str) -> Result<Vec<ScanRecord>, CoreError> {
        let provider_id = provider_id.to_owned();
        self.with_connection(move |connection| {
            let mut statement = connection.prepare("SELECT record_json FROM scan_runs WHERE provider_id=?1 ORDER BY started_at,job_id").map_err(storage)?;
            let rows = statement.query_map([provider_id],|r|r.get::<_,String>(0)).map_err(storage)?;
            rows.map(|row|parse(&row.map_err(storage)?)).collect()
        }).await
    }
}
