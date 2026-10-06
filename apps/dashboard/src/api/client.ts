import type { ApiInfo, ProviderSummary, StartScanRequest, StartScanResult, ScanSummary, OverviewQuery, OverviewResult, SessionQuery, SessionPage, ExportRequest, ExportResult, SettingsResult, UpdateSettingsRequest, ChooseProviderDirectoryResult } from './generated/usage';
import type { AutoCollectionStatus, UpdateAutoCollectionRequest } from './generated/usage';
export type UsageEvent = { kind: 'scan'; scan: ScanSummary } | { kind: 'auto'; status: AutoCollectionStatus } | { kind: 'resync' };
/** Implementations reject with ApiError. Negotiate apiVersion before queries. Polling/events stay inside transports. */
export interface UsageClient {
  getApiInfo(): Promise<ApiInfo>;
  listProviders(): Promise<ProviderSummary[]>;
  startScan(request: StartScanRequest): Promise<StartScanResult>;
  getScan(jobId: string): Promise<ScanSummary>;
  cancelScan(jobId: string): Promise<void>;
  getOverview(query: OverviewQuery): Promise<OverviewResult>;
  listSessions(query: SessionQuery): Promise<SessionPage>;
  exportUsage(request: ExportRequest, language?: 'zh' | 'en'): Promise<ExportResult>;
  /** API 1.1 extension: callers check capabilities and method presence for older clients. */
  getSettings?(): Promise<SettingsResult>;
  updateSettings?(request: UpdateSettingsRequest): Promise<SettingsResult>;
  chooseProviderDirectory?(providerId: string, language?: 'zh' | 'en'): Promise<ChooseProviderDirectoryResult>;
  getAutoCollection?(): Promise<AutoCollectionStatus>;
  updateAutoCollection?(request: UpdateAutoCollectionRequest): Promise<AutoCollectionStatus>;
  subscribeUsage?(listener: (event: UsageEvent) => void): Promise<() => void>;
}
