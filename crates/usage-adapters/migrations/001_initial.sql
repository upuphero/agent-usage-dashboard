CREATE TABLE schema_migrations (version INTEGER PRIMARY KEY CHECK(version > 0), checksum TEXT NOT NULL) STRICT;
CREATE TABLE devices (device_id TEXT PRIMARY KEY NOT NULL) STRICT;
CREATE TABLE source_datasets (
    dataset_id TEXT PRIMARY KEY NOT NULL,
    provider_id TEXT NOT NULL,
    product_id TEXT NOT NULL,
    origin_device_id TEXT NOT NULL REFERENCES devices(device_id)
) STRICT;
CREATE TABLE report_snapshots (
    snapshot_key TEXT PRIMARY KEY NOT NULL,
    dataset_id TEXT NOT NULL REFERENCES source_datasets(dataset_id),
    provider_id TEXT NOT NULL,
    product_id TEXT NOT NULL,
    report_kind TEXT NOT NULL CHECK(report_kind IN ('daily','session')),
    timezone TEXT NOT NULL,
    scope_key TEXT NOT NULL,
    revision INTEGER NOT NULL CHECK(revision > 0),
    metadata_json TEXT NOT NULL
) STRICT;
CREATE TABLE report_rows (
    snapshot_key TEXT NOT NULL REFERENCES report_snapshots(snapshot_key) ON DELETE CASCADE,
    row_key TEXT NOT NULL,
    dimension_kind TEXT NOT NULL CHECK(dimension_kind IN ('day','session')),
    local_date TEXT,
    session_id TEXT,
    model_key TEXT NOT NULL,
    model_id TEXT,
    model_vendor TEXT,
    input_uncached INTEGER CHECK(input_uncached >= 0), input_accuracy TEXT NOT NULL,
    cache_read INTEGER CHECK(cache_read >= 0), cache_read_accuracy TEXT NOT NULL,
    cache_write INTEGER CHECK(cache_write >= 0), cache_write_accuracy TEXT NOT NULL,
    output_total INTEGER CHECK(output_total >= 0), output_accuracy TEXT NOT NULL,
    output_reasoning INTEGER CHECK(output_reasoning >= 0), reasoning_accuracy TEXT NOT NULL,
    total INTEGER CHECK(total >= 0), total_accuracy TEXT NOT NULL,
    amount_usd TEXT, cost_accuracy TEXT NOT NULL,
    pricing_version TEXT, pricing_as_of TEXT, missing_models_json TEXT NOT NULL,
    session_started_at TEXT, last_activity_at TEXT,
    PRIMARY KEY(snapshot_key,row_key),
    CHECK((dimension_kind='day' AND local_date IS NOT NULL AND session_id IS NULL) OR
          (dimension_kind='session' AND session_id IS NOT NULL AND local_date IS NULL)),
    CHECK((input_uncached IS NULL) = (input_accuracy='unavailable')),
    CHECK((cache_read IS NULL) = (cache_read_accuracy='unavailable')),
    CHECK((cache_write IS NULL) = (cache_write_accuracy='unavailable')),
    CHECK((output_total IS NULL) = (output_accuracy='unavailable')),
    CHECK((output_reasoning IS NULL) = (reasoning_accuracy='unavailable')),
    CHECK((total IS NULL) = (total_accuracy='unavailable')),
    CHECK((amount_usd IS NULL) = (cost_accuracy='unavailable'))
) STRICT;
CREATE INDEX snapshots_query ON report_snapshots(provider_id,report_kind,timezone,scope_key);
CREATE INDEX snapshots_product ON report_snapshots(product_id,dataset_id);
CREATE INDEX rows_day ON report_rows(local_date,model_key,snapshot_key) WHERE dimension_kind='day';
CREATE INDEX rows_session ON report_rows(session_id,model_key,snapshot_key) WHERE dimension_kind='session';
CREATE TABLE scan_runs (
    job_id TEXT PRIMARY KEY NOT NULL,
    provider_id TEXT NOT NULL,
    started_at TEXT NOT NULL,
    record_json TEXT NOT NULL
) STRICT;
CREATE INDEX scans_provider ON scan_runs(provider_id,started_at,job_id);
