// GENERATED from crates/usage-contracts/src/lib.rs. Do not edit.
export const API_VERSION = "1.2.0" as const;
export const SCAN_EVENT = "usage://scan-updated" as const;
export const AUTO_COLLECTION_EVENT = "usage://auto-collection-updated" as const;
export const COMMANDS = {
  getApiInfo: "get_api_info",
  listProviders: "list_providers",
  startScan: "start_scan",
  getScan: "get_scan",
  cancelScan: "cancel_scan",
  getOverview: "get_overview",
  listSessions: "list_sessions",
  exportUsage: "export_usage",
  getSettings: "get_settings",
  updateSettings: "update_settings",
  chooseProviderDirectory: "choose_provider_directory",
  getAutoCollection: "get_auto_collection",
  updateAutoCollection: "update_auto_collection",
} as const;
export type Accuracy = "exact" | "derived" | "estimated" | "unavailable";
export type ReportKind = "daily" | "session";
export type CoverageState = "complete" | "partial" | "archived";
export type ProviderState = "not-detected" | "no-data" | "ready" | "scanning" | "permission-denied" | "schema-unsupported" | "partial" | "stale" | "error";
export type ScanState = "queued" | "running" | "succeeded" | "failed" | "cancelled";
export type Bucket = "day" | "week" | "month";
export type ExportFormat = "json" | "csv";
export type CostKind = "api-equivalent-estimate";
export type ErrorCode = "INVALID_QUERY" | "INVALID_DATA" | "PROVIDER_NOT_FOUND" | "PROVIDER_DISABLED" | "SOURCE_NOT_DETECTED" | "PERMISSION_DENIED" | "SCHEMA_UNSUPPORTED" | "COVERAGE_INCOMPLETE" | "TIMEOUT" | "OUTPUT_LIMIT_EXCEEDED" | "CANCELLED" | "STORAGE" | "STORAGE_SCHEMA_NEWER" | "UNSUPPORTED_FILTER" | "DATASET_CONFLICT" | "OVERFLOW" | "SCAN_NOT_FOUND" | "SCAN_BUSY" | "SHUTTING_DOWN" | "COLLECTION_FAILED" | "API_VERSION_UNSUPPORTED" | "EXPORT_FAILED" | "INTERNAL" | "SETTINGS_CONFLICT" | "INVALID_DIRECTORY_REF";
export const ERROR_CODES = ["INVALID_QUERY", "INVALID_DATA", "PROVIDER_NOT_FOUND", "PROVIDER_DISABLED", "SOURCE_NOT_DETECTED", "PERMISSION_DENIED", "SCHEMA_UNSUPPORTED", "COVERAGE_INCOMPLETE", "TIMEOUT", "OUTPUT_LIMIT_EXCEEDED", "CANCELLED", "STORAGE", "STORAGE_SCHEMA_NEWER", "UNSUPPORTED_FILTER", "DATASET_CONFLICT", "OVERFLOW", "SCAN_NOT_FOUND", "SCAN_BUSY", "SHUTTING_DOWN", "COLLECTION_FAILED", "API_VERSION_UNSUPPORTED", "EXPORT_FAILED", "INTERNAL", "SETTINGS_CONFLICT", "INVALID_DIRECTORY_REF"] as const;
export type AutoCollectionState = "disabled" | "idle" | "checking" | "scanning" | "waiting" | "backoff";
export interface Metric<T> {
  value: T | null;
  accuracy: Accuracy;
  knownRows: number;
  missingRows: number;
}
export interface TokenMetrics {
  inputUncached: Metric<string>;
  cacheRead: Metric<string>;
  cacheWrite: Metric<string>;
  outputTotal: Metric<string>;
  outputReasoning: Metric<string>;
  total: Metric<string>;
}
export interface CostEstimate {
  amountUsd: Metric<string>;
  kind: CostKind;
  pricingVersions: Array<string>;
  pricingAsOf: string | null;
  missingModels: Array<string>;
}
export interface UsageAggregate {
  tokens: TokenMetrics;
  cost: CostEstimate;
}
export interface DateRange {
  start: string;
  end: string;
}
export interface Coverage {
  state: CoverageState;
  range: DateRange | null;
  observedFrom: string | null;
  observedUntil: string | null;
}
export interface ProviderCapabilities {
  reportKinds: Array<ReportKind>;
  supportedDimensions: Array<string>;
  supportedMetrics: Array<string>;
  supportsDateSessionIntersection: boolean;
  supportsIncrementalCollection: boolean;
  supportsQuota: boolean;
}
export interface ApiInfo {
  apiVersion: string;
  appVersion: string;
  capabilities: Array<string>;
}
export interface ApiError {
  apiVersion: string;
  code: ErrorCode;
  message: string;
  retryable: boolean;
}
export interface ProviderSummary {
  providerId: string;
  productId: string;
  displayName: string;
  enabled: boolean;
  state: ProviderState;
  pathHint: string | null;
  capabilities: ProviderCapabilities;
  lastSuccessAt: string | null;
  coverage: Array<Coverage>;
  lastScan: ScanSummary | null;
}
export interface StartScanRequest {
  providerId: string;
  timezone: string;
}
export interface StartScanResult {
  jobId: string;
}
export interface JobRequest {
  jobId: string;
}
export interface ScanSummary {
  apiVersion: string;
  jobId: string;
  providerId: string;
  state: ScanState;
  startedAt: string;
  finishedAt: string | null;
  error: ApiError | null;
  snapshotsReplaced: number;
  rowsWritten: number;
}
export interface OverviewQuery {
  range: DateRange;
  timezone: string;
  providerIds: Array<string>;
  modelIds: Array<string>;
  bucket: Bucket;
}
export interface UsageGroup {
  id: string;
  usage: UsageAggregate;
}
export interface TimeBucket {
  start: string;
  usage: UsageAggregate;
}
export interface OverviewResult {
  apiVersion: string;
  query: OverviewQuery;
  usage: UsageAggregate;
  byProvider: Array<UsageGroup>;
  byModel: Array<UsageGroup>;
  buckets: Array<TimeBucket>;
  coverage: Array<Coverage>;
  warnings: Array<string>;
  stale: boolean;
  lastSuccessAt: string | null;
}
export interface SessionQuery {
  timezone: string;
  providerIds: Array<string>;
  modelIds: Array<string>;
  activeRange: DateRange | null;
  offset: number;
  limit: number;
}
export interface SessionItem {
  sessionId: string;
  providerId: string;
  productId: string;
  sourceDatasetId: string;
  originDeviceId: string;
  modelId: string | null;
  modelVendor: string | null;
  startedAt: string | null;
  lastActivityAt: string | null;
  usage: UsageAggregate;
}
export interface SessionPage {
  apiVersion: string;
  items: Array<SessionItem>;
  total: number;
  nextOffset: number | null;
  stale: boolean;
  warnings: Array<string>;
  dateFilterSemantics: string;
}
export interface ExportRequest {
  format: ExportFormat;
  query: OverviewQuery;
}
export interface ExportResult {
  apiVersion: string;
  exportId: string;
  suggestedFilename: string;
  mediaType: string;
  byteLength: string;
}
export interface ArchiveScope {
  kind: string;
  range: DateRange | null;
  modelIds: Array<string>;
}
export interface ArchiveRow {
  date: string | null;
  sessionId: string | null;
  modelId: string | null;
  modelVendor: string | null;
  usage: UsageAggregate;
  startedAt: string | null;
  lastActivityAt: string | null;
}
export interface ArchiveSnapshot {
  providerId: string;
  productId: string;
  sourceDatasetId: string;
  originDeviceId: string;
  reportKind: ReportKind;
  timezone: string;
  scope: ArchiveScope;
  revision: string;
  collectedAt: string;
  collectionStartedAt: string;
  collectorVersion: string;
  normalizationVersion: string;
  coverage: Coverage;
  warnings: Array<string>;
  rows: Array<ArchiveRow>;
}
export interface UsageArchive {
  archiveSchemaVersion: string;
  appVersion: string;
  exportedAt: string;
  snapshots: Array<ArchiveSnapshot>;
}
export interface SourceDirectory {
  directoryRef: string;
  label: string;
}
export interface ProviderSettings {
  providerId: string;
  enabled: boolean;
  directory: SourceDirectory | null;
}
export interface SettingsResult {
  apiVersion: string;
  revision: string;
  timezone: string;
  providers: Array<ProviderSettings>;
  collectionNotice: string;
  directoryChangePolicy: string;
}
export interface ProviderSettingsUpdate {
  providerId: string;
  enabled: boolean;
  directoryRef: string | null;
}
export interface UpdateSettingsRequest {
  expectedRevision: string;
  timezone: string;
  providers: Array<ProviderSettingsUpdate>;
}
export interface ChooseProviderDirectoryRequest {
  providerId: string;
}
export interface ChooseProviderDirectoryResult {
  apiVersion: string;
  providerId: string;
  directory: SourceDirectory | null;
}
export interface AutoCollectionConfig {
  enabled: boolean;
  intervalMinutes: number;
}
export interface UpdateAutoCollectionRequest {
  expectedRevision: string;
  config: AutoCollectionConfig;
}
export interface AutoProviderStatus {
  providerId: string;
  state: AutoCollectionState;
  jobId: string | null;
  lastSuccessAt: string | null;
  nextCheckAt: string | null;
  watching: boolean;
  error: ApiError | null;
}
export interface AutoCollectionStatus {
  apiVersion: string;
  revision: string;
  config: AutoCollectionConfig;
  timezone: string;
  providers: Array<AutoProviderStatus>;
}
