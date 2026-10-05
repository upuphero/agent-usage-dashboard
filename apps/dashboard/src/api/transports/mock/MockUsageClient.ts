import type { UsageClient } from '../../client';
import {
  API_VERSION, type ApiInfo, type ProviderSummary, type StartScanRequest, type ScanSummary,
  type OverviewQuery, type OverviewResult, type SessionQuery, type SessionPage, type ExportRequest,
  type ExportResult, type UsageAggregate, type SettingsResult, type UpdateSettingsRequest, type ChooseProviderDirectoryResult,
} from '../../generated/usage';
import { apiError, assertApiVersion, assertResponseVersion } from '../../protocol';
import { delay, isActiveScan, waitForScan } from '../scan';
import {
  ALL_USAGE, TODAY_USAGE, MONTH_USAGE, EMPTY_USAGE, PROVIDERS, PROVIDER_USAGE, MODEL_PROVIDER,
  DAILY_TOTALS, DAILY_PROVIDER_TOTALS, SEPTEMBER_PROVIDER_TOTALS, TODAY_PROVIDER_USAGE,
  MONTH_PROVIDER_USAGE, DEMO_API_INFO, DEMO_DATE, DEMO_RANGE, DEMO_TIMEZONE, DEMO_UPDATED,
  SESSIONS, reported, unavailable,
} from './fixtures';

export const DEMO_SCENARIOS = ['partial', 'stale', 'empty', 'error', 'loading', 'scan-error', 'limited', 'version-mismatch'] as const;
export type DemoScenario = typeof DEMO_SCENARIOS[number];

interface DemoJob { summary: ScanSummary; clock: number; timezone: string }

export class MockUsageClient implements UsageClient {
  private readonly jobs = new Map<string, DemoJob>();
  private counter = 0;
  private scanFailed = false;
  private readonly selectedDirectories = new Map<string, string>();
  private directoryCounter = 0;
  private settings: SettingsResult = {
    apiVersion: API_VERSION, revision: '1', timezone: DEMO_TIMEZONE,
    providers: PROVIDERS.map(provider => ({ providerId: provider.providerId, enabled: provider.enabled, directory: null })),
    collectionNotice: '演示设置只改变合成来源开关，不读取日志、不保存到磁盘。', directoryChangePolicy: 'preserve-dataset',
  };

  constructor(readonly scenario: DemoScenario = 'partial', private readonly latencyMs = 180, private readonly scanMs = 1500) {}

  private async readDelay(): Promise<void> { await delay(this.scenario === 'loading' ? 4000 : this.latencyMs); }
  private checkVersion(): void { assertApiVersion(this.scenario === 'version-mismatch' ? '2.0.0' : API_VERSION); }
  private checkTimezone(timezone: string): void {
    this.checkVersion();
    if (timezone !== DEMO_TIMEZONE) throw apiError('UNSUPPORTED_FILTER', '演示快照仅提供 America/Phoenix 时区，请恢复该时区。');
  }
  private checkProviders(ids: string[]): void {
    if (ids.some(id => !PROVIDERS.some(provider => provider.providerId === id))) throw apiError('PROVIDER_NOT_FOUND', '未找到该来源。');
  }
  private readJob(jobId: string): ScanSummary {
    this.checkVersion();
    const job = this.jobs.get(jobId);
    if (!job) throw apiError('SCAN_NOT_FOUND', '未找到扫描任务。');
    if (isActiveScan(job.summary)) {
      const elapsed = Date.now() - job.clock;
      if (elapsed >= this.scanMs) {
        const failed = this.scenario === 'scan-error';
        job.summary = {
          ...job.summary, state: failed ? 'failed' : 'succeeded', finishedAt: new Date().toISOString(),
          error: failed ? apiError('COLLECTION_FAILED', '演示扫描失败；上次成功的快照已保留。', true) : null,
          snapshotsReplaced: failed ? 0 : 2, rowsWritten: failed ? 0 : 9,
        };
        this.scanFailed = failed;
      } else if (elapsed >= this.scanMs / 4) job.summary.state = 'running';
    }
    return structuredClone(job.summary);
  }

  async getApiInfo(): Promise<ApiInfo> {
    await delay(this.latencyMs);
    const info: ApiInfo = structuredClone(DEMO_API_INFO);
    if (this.scenario === 'version-mismatch') info.apiVersion = '2.0.0';
    return assertResponseVersion(info);
  }

  async listProviders(): Promise<ProviderSummary[]> {
    this.checkVersion();
    await this.readDelay();
    return PROVIDERS.map(original => {
      const provider = structuredClone(original);
      provider.enabled = this.settings.providers.find(item => item.providerId === provider.providerId)!.enabled;
      const job = [...this.jobs.values()].reverse().find(item => item.summary.providerId === provider.providerId);
      if (job) {
        provider.lastScan = this.readJob(job.summary.jobId);
        if (isActiveScan(provider.lastScan)) provider.state = 'scanning';
        else if (provider.lastScan.state === 'failed') provider.state = 'stale';
      } else if (this.scenario === 'stale') provider.state = 'stale';
      else if (this.scenario === 'empty') { provider.state = 'no-data'; provider.coverage = provider.coverage.map(coverage => ({ ...coverage, state: 'complete' })); }
      else if (this.scenario === 'error') provider.state = 'permission-denied';
      if (this.scenario === 'limited') {
        provider.capabilities.supportedDimensions = ['day'];
        provider.capabilities.reportKinds = ['daily'];
      }
      return provider;
    });
  }

  async startScan(request: StartScanRequest): Promise<{ jobId: string }> {
    this.checkProviders([request.providerId]);
    await delay(this.latencyMs);
    for (const job of this.jobs.values()) {
      if (job.summary.providerId === request.providerId && isActiveScan(this.readJob(job.summary.jobId))) {
        if (job.timezone !== request.timezone) throw apiError('SCAN_BUSY', '此来源有其他时区的活动扫描，请等待任务完成。', true);
        return { jobId: job.summary.jobId };
      }
    }
    this.checkTimezone(request.timezone);
    if (!this.settings.providers.find(provider => provider.providerId === request.providerId)!.enabled) throw apiError('PROVIDER_DISABLED', '该演示来源已关闭。');
    const jobId = `demo-scan-${++this.counter}`;
    this.jobs.set(jobId, {
      clock: Date.now(), timezone: request.timezone,
      summary: { apiVersion: API_VERSION, jobId, providerId: request.providerId, state: 'queued', startedAt: new Date().toISOString(), finishedAt: null, error: null, snapshotsReplaced: 0, rowsWritten: 0 },
    });
    return { jobId };
  }
  getScan(jobId: string): Promise<ScanSummary> {
    return waitForScan(async () => this.readJob(jobId), Math.max(1, this.latencyMs));
  }
  async cancelScan(jobId: string): Promise<void> {
    const summary = this.readJob(jobId);
    if (isActiveScan(summary)) {
      const job = this.jobs.get(jobId)!;
      job.summary = { ...summary, state: 'cancelled', finishedAt: new Date().toISOString() };
    }
  }

  async getOverview(query: OverviewQuery): Promise<OverviewResult> {
    this.checkTimezone(query.timezone);
    this.checkProviders(query.providerIds);
    if (query.providerIds.length > 1) throw apiError('UNSUPPORTED_FILTER', '演示总览支持全部来源或单个来源。');
    if (query.range.start >= query.range.end) throw apiError('INVALID_QUERY', '日期范围必须满足开始日期早于结束日期。');
    const recent30 = query.range.start === '2026-09-05' && query.range.end === DEMO_RANGE.end;
    const week = (query.range.start === DEMO_RANGE.start && query.range.end === DEMO_RANGE.end) || recent30;
    const today = query.range.start === DEMO_DATE && query.range.end === DEMO_RANGE.end;
    const month = query.range.start === '2026-10-01' && query.range.end === '2026-11-01';
    if (!week && !today && !month) throw apiError('UNSUPPORTED_FILTER', '演示数据只提供今日、本周、最近30天和本月预置范围。');
    const selectedProviders = query.providerIds.length ? query.providerIds : PROVIDERS.map(p => p.providerId);
    if (query.modelIds.length > 1 || query.modelIds.some(id => !MODEL_PROVIDER[id])) throw apiError('UNSUPPORTED_FILTER', '该模型筛选没有可用的演示快照。');
    if (query.modelIds.length && (this.scenario === 'limited' || selectedProviders.some(id => !PROVIDERS.find(p => p.providerId === id)!.capabilities.supportedDimensions.includes('model')))) {
      throw apiError('UNSUPPORTED_FILTER', '所选来源不支持模型筛选。');
    }
    await this.readDelay();
    if (this.scenario === 'error') throw apiError('PERMISSION_DENIED', '无法读取来源。请检查访问权限；已有快照不会被清空。', true);

    const fixtureSet = today ? TODAY_PROVIDER_USAGE : month ? MONTH_PROVIDER_USAGE : PROVIDER_USAGE;
    const modelProvider = query.modelIds.length ? MODEL_PROVIDER[query.modelIds[0]] : undefined;
    const ids = modelProvider ? selectedProviders.filter(id => id === modelProvider) : selectedProviders;
    const empty = this.scenario === 'empty' || ids.length === 0;
    const single = ids.length === 1 ? ids[0] : undefined;
    const usage = empty ? EMPTY_USAGE : single ? fixtureSet[single] : today ? TODAY_USAGE : month ? MONTH_USAGE : ALL_USAGE;
    const trendUsage = (total: string): UsageAggregate => ({ tokens: { ...EMPTY_USAGE.tokens, total: reported(total) }, cost: { ...EMPTY_USAGE.cost, amountUsd: unavailable() } });
    const daily = DAILY_TOTALS.flatMap(([start, total], index) => start >= query.range.start && start < query.range.end
      ? [{ start, usage: trendUsage(single ? DAILY_PROVIDER_TOTALS[single][index] : total) }] : []);
    const buckets = query.bucket === 'day' ? daily : query.bucket === 'week'
      ? [{ start: '2026-09-28', usage: trendUsage(usage.tokens.total.value!) }]
      : week ? [
        { start: '2026-09-01', usage: trendUsage(single ? SEPTEMBER_PROVIDER_TOTALS[single] : '518000') },
        { start: '2026-10-01', usage: trendUsage(single ? MONTH_PROVIDER_USAGE[single].tokens.total.value! : '962000') },
      ] : [{ start: '2026-10-01', usage: trendUsage(usage.tokens.total.value!) }];
    return structuredClone({
      apiVersion: API_VERSION, query, usage,
      byProvider: empty ? [] : ids.map(id => ({ id, usage: fixtureSet[id] })),
      byModel: empty ? [] : Object.entries(MODEL_PROVIDER).filter(([, id]) => ids.includes(id)).map(([id, provider]) => ({ id, usage: fixtureSet[provider] })),
      buckets: empty ? [] : buckets,
      coverage: (empty ? selectedProviders : ids).map(id => empty ? { ...PROVIDERS.find(p => p.providerId === id)!.coverage[0], state: 'complete' as const, range: query.range } : PROVIDERS.find(p => p.providerId === id)!.coverage[0]),
      warnings: ['DEMO_DATA', ...(!empty && ids.includes('ccusage.antigravity') ? ['MODEL_BREAKDOWN_UNAVAILABLE', 'MISSING_PRICING'] : [])],
      stale: this.scenario === 'stale' || this.scanFailed, lastSuccessAt: DEMO_UPDATED,
    });
  }

  async listSessions(query: SessionQuery): Promise<SessionPage> {
    this.checkTimezone(query.timezone);
    this.checkProviders(query.providerIds);
    if (!Number.isInteger(query.offset) || query.offset < 0 || !Number.isInteger(query.limit) || query.limit < 1 || query.limit > 200) throw apiError('INVALID_QUERY', '分页参数无效。');
    if (this.scenario === 'limited' || query.providerIds.some(id => !PROVIDERS.find(p => p.providerId === id)!.capabilities.reportKinds.includes('session'))) throw apiError('UNSUPPORTED_FILTER', '所选来源不支持会话报表。');
    const selected = query.providerIds.length ? PROVIDERS.filter(provider => query.providerIds.includes(provider.providerId)) : PROVIDERS.filter(provider => provider.capabilities.reportKinds.includes('session'));
    if (query.modelIds.some(id => !MODEL_PROVIDER[id]) || (query.modelIds.length > 0 && selected.some(provider => !provider.capabilities.supportedDimensions.includes('model')))) throw apiError('UNSUPPORTED_FILTER', '所选来源不支持该模型筛选。');
    if (query.activeRange && query.activeRange.start >= query.activeRange.end) throw apiError('INVALID_QUERY', '活动日期范围无效。');
    await this.readDelay();
    if (this.scenario === 'error') throw apiError('PERMISSION_DENIED', '无法读取会话数据，请检查来源权限。', true);
    const items = this.scenario === 'empty' ? [] : SESSIONS.filter(session =>
      (!query.providerIds.length || query.providerIds.includes(session.providerId)) &&
      (!query.modelIds.length || (session.modelId !== null && query.modelIds.includes(session.modelId))) &&
      (!query.activeRange || (session.lastActivityAt !== null && session.lastActivityAt.slice(0, 10) >= query.activeRange.start && session.lastActivityAt.slice(0, 10) < query.activeRange.end)),
    );
    return structuredClone({ apiVersion: API_VERSION, items: items.slice(query.offset, query.offset + query.limit), total: items.length,
      nextOffset: query.offset + query.limit < items.length ? query.offset + query.limit : null,
      stale: this.scenario === 'stale' || this.scanFailed, warnings: ['DEMO_DATA'], dateFilterSemantics: 'active-sessions-lifetime-usage' });
  }

  async exportUsage(request: ExportRequest): Promise<ExportResult> {
    this.checkTimezone(request.query.timezone);
    this.checkProviders(request.query.providerIds);
    if (request.format === 'json' && request.query.modelIds.length) throw apiError('UNSUPPORTED_FILTER', '完整历史归档不支持模型筛选。');
    await delay(this.latencyMs);
    return { apiVersion: API_VERSION, exportId: 'demo-export-no-file-written', suggestedFilename: `demo-usage.${request.format === 'json' ? 'aiusage.json' : 'csv'}`, mediaType: request.format === 'json' ? 'application/json' : 'text/csv', byteLength: '0' };
  }
  async getSettings(): Promise<SettingsResult> { this.checkVersion(); await delay(this.latencyMs); return structuredClone(this.settings); }
  async chooseProviderDirectory(providerId: string): Promise<ChooseProviderDirectoryResult> {
    this.checkVersion(); this.checkProviders([providerId]); await delay(this.latencyMs);
    const directoryRef = `demo-directory-${++this.directoryCounter}`; this.selectedDirectories.set(directoryRef, providerId);
    return { apiVersion: API_VERSION, providerId, directory: { directoryRef, label: '合成演示目录 · 未打开系统选择器' } };
  }
  async updateSettings(request: UpdateSettingsRequest): Promise<SettingsResult> {
    this.checkTimezone(request.timezone);
    if ([...this.jobs.values()].some(job => isActiveScan(this.readJob(job.summary.jobId)))) throw apiError('SCAN_BUSY', '扫描期间不能修改设置。', true);
    if (request.expectedRevision !== this.settings.revision) throw apiError('SETTINGS_CONFLICT', '设置已变化，请重新读取。');
    this.checkProviders(request.providers.map(provider => provider.providerId));
    if (new Set(request.providers.map(provider => provider.providerId)).size !== request.providers.length) throw apiError('INVALID_QUERY', '来源设置重复。');
    const next = structuredClone(this.settings);
    for (const update of request.providers) {
      const target = next.providers.find(provider => provider.providerId === update.providerId)!;
      if (update.directoryRef !== null && update.directoryRef !== target.directory?.directoryRef && this.selectedDirectories.get(update.directoryRef) !== update.providerId) throw apiError('INVALID_DIRECTORY_REF', '请重新选择目录。');
      target.enabled = update.enabled;
      target.directory = update.directoryRef === null ? null : { directoryRef: update.directoryRef, label: '合成演示目录 · 未读取日志' };
    }
    next.revision = (BigInt(next.revision) + 1n).toString(); this.settings = next;
    return structuredClone(next);
  }
}
