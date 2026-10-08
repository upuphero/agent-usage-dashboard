import type { UsageClient, UsageEvent } from '../../client';
import {
  API_VERSION, type ApiInfo, type ProviderSummary, type StartScanRequest, type ScanSummary,
  type OverviewQuery, type OverviewResult, type SessionQuery, type SessionPage, type ExportRequest,
  type ExportResult, type UsageAggregate, type SettingsResult, type UpdateSettingsRequest, type ChooseProviderDirectoryResult,
  type AutoCollectionStatus, type AutoCollectionConfig, type UpdateAutoCollectionRequest,
  type ApiError, type TimezoneMode, type TimezoneRebuildState, type TimezoneProviderStatus, type TimezoneStatus, type UpdateTimezoneRequest,
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

interface DemoJob { summary: ScanSummary; clock: number; timezone: string; automatic: boolean; generation: number }
const REBUILD_RETRY_MS = 60_000;
/** Host-style canonicalization (case and aliases); null for anything that is not an IANA zone. */
function canonicalZone(zone: string): string | null {
  try { return new Intl.DateTimeFormat('en-US', { timeZone: zone }).resolvedOptions().timeZone; } catch { return null; }
}

export class MockUsageClient implements UsageClient {
  private readonly jobs = new Map<string, DemoJob>();
  private counter = 0;
  private scanFailed = false;
  private readonly selectedDirectories = new Map<string, string>();
  private directoryCounter = 0;
  private autoConfig: AutoCollectionConfig = { enabled: false, intervalMinutes: 5 };
  private listeners = new Set<(event: UsageEvent) => void>();
  private autoTimer: ReturnType<typeof setTimeout> | undefined;
  private autoDeadline = 0;
  private readonly completions = new Map<string, ReturnType<typeof setTimeout>>();
  private readonly generations = new Map<string, number>();
  private readonly baselines = new Map<string, number>();
  private readonly eligibleAt = new Map<string, number>();
  private settings: SettingsResult = {
    apiVersion: API_VERSION, revision: '1', timezone: DEMO_TIMEZONE,
    providers: PROVIDERS.map(provider => ({ providerId: provider.providerId, enabled: provider.enabled, directory: null })),
    collectionNotice: '演示设置只改变合成来源开关，不读取日志、不保存到磁盘。', directoryChangePolicy: 'preserve-dataset',
  };
  // API 1.3 timezone state. settings.timezone is always the effective zone; rebuild jobs are tracked by provider jobId.
  private timezoneMode: TimezoneMode = 'follow-system';
  private systemZone: string | null = DEMO_TIMEZONE;
  private detectionError: ApiError | null = null;
  private pendingZone: string | null = null;
  private rebuild: TimezoneRebuildState = 'idle';
  private rebuildProviders: TimezoneProviderStatus[] = [];
  private retryAt: number | null = null;
  private retryTimer: ReturnType<typeof setTimeout> | undefined;
  private timezoneSequence = 0n;
  private timezoneSignature: string;

  constructor(readonly scenario: DemoScenario = 'partial', private readonly latencyMs = 180, private readonly scanMs = 1500) {
    this.timezoneSignature = JSON.stringify(this.timezoneStatus());
  }

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
        if (job.automatic) {
          if (!failed) this.baselines.set(job.summary.providerId, job.generation);
          this.eligibleAt.set(job.summary.providerId, Date.now() + this.autoConfig.intervalMinutes * 60000);
          this.scheduleCheck();
        }
        this.emit({ kind: 'scan', scan: structuredClone(job.summary) });
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

  async startScan(request: StartScanRequest, automatic = false): Promise<{ jobId: string }> {
    this.checkProviders([request.providerId]);
    await delay(this.latencyMs);
    for (const job of this.jobs.values()) {
      if (job.summary.providerId === request.providerId && isActiveScan(this.readJob(job.summary.jobId))) {
        if (job.timezone !== request.timezone) throw apiError('SCAN_BUSY', '此来源有其他时区的活动扫描，请等待任务完成。', true);
        if (!automatic) job.automatic = false;
        return { jobId: job.summary.jobId };
      }
    }
    // Scans in the effective zone are accepted even when a simulated system zone has no synthetic data.
    if (request.timezone === this.settings.timezone) this.checkVersion(); else this.checkTimezone(request.timezone);
    if (!this.settings.providers.find(provider => provider.providerId === request.providerId)!.enabled) throw apiError('PROVIDER_DISABLED', '该演示来源已关闭。');
    return { jobId: this.createJob(request.providerId, request.timezone, automatic) };
  }
  private createJob(providerId: string, timezone: string, automatic: boolean): string {
    const jobId = `demo-scan-${++this.counter}`;
    this.jobs.set(jobId, {
      clock: Date.now(), timezone, automatic, generation: this.generations.get(providerId) ?? 0,
      summary: { apiVersion: API_VERSION, jobId, providerId, state: 'queued', startedAt: new Date().toISOString(), finishedAt: null, error: null, snapshotsReplaced: 0, rowsWritten: 0 },
    });
    this.emit({ kind: 'scan', scan: structuredClone(this.jobs.get(jobId)!.summary) });
    this.completions.set(jobId, setTimeout(() => { this.completions.delete(jobId); this.readJob(jobId); this.tick(); void this.getAutoCollection().then(status => this.emit({ kind: 'auto', status })); }, this.scanMs));
    return jobId;
  }
  private scanActive(): boolean { return [...this.jobs.values()].some(job => isActiveScan(this.readJob(job.summary.jobId))); }
  getScan(jobId: string): Promise<ScanSummary> {
    return waitForScan(async () => this.readJob(jobId), Math.max(1, this.latencyMs));
  }
  async cancelScan(jobId: string): Promise<void> {
    const summary = this.readJob(jobId);
    if (isActiveScan(summary)) {
      const job = this.jobs.get(jobId)!;
      job.summary = { ...summary, state: 'cancelled', finishedAt: new Date().toISOString() };
      this.eligibleAt.set(summary.providerId, Date.now() + this.autoConfig.intervalMinutes * 60000);
      this.emit({ kind: 'scan', scan: structuredClone(job.summary) });
    }
    this.tick();
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
      stale: this.scenario === 'stale' || this.scanFailed, lastSuccessAt: [...this.jobs.values()].reverse().find(job => job.summary.state === 'succeeded' && selectedProviders.includes(job.summary.providerId))?.summary.finishedAt ?? DEMO_UPDATED,
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
    // Legacy write: the effective zone keeps the mode; a different zone means "fixed at that zone" (demo-zone rule applies).
    const zone = request.timezone === this.settings.timezone ? request.timezone : canonicalZone(request.timezone) ?? request.timezone;
    if (zone === this.settings.timezone) this.checkVersion(); else this.checkTimezone(zone);
    if (this.scanActive()) throw apiError('SCAN_BUSY', '扫描期间不能修改设置。', true);
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
    next.revision = (BigInt(next.revision) + 1n).toString();
    const moved = zone !== next.timezone; next.timezone = zone; this.settings = next;
    if (moved) { this.timezoneMode = 'fixed'; this.resetRebuild(); }
    this.baselines.clear(); this.scheduleCheck(); this.tick();
    return structuredClone(this.settings);
  }
  private emit(event: UsageEvent) { for (const listener of this.listeners) listener(event); }
  /** Synthetic source edit for browser/transport acceptance. Never reads local files. */
  simulateSourceChange(providerId: string) {
    this.generations.set(providerId, (this.generations.get(providerId) ?? 0) + 1); this.scheduleCheck();
  }
  private scheduleCheck(wait = 2000) {
    if (!this.autoConfig.enabled) return;
    const deadline = Date.now() + wait;
    if (this.autoTimer !== undefined) { if (deadline >= this.autoDeadline) return; clearTimeout(this.autoTimer); }
    this.autoDeadline = deadline;
    this.autoTimer = setTimeout(() => { this.autoTimer = undefined; void this.checkAutomatically(); }, wait);
  }
  private async checkAutomatically() {
    if (!this.autoConfig.enabled) return;
    if (!this.scanActive()) {
      const source = this.settings.providers.find(provider => provider.enabled && Date.now() >= (this.eligibleAt.get(provider.providerId) ?? 0)
        && this.baselines.get(provider.providerId) !== (this.generations.get(provider.providerId) ?? 0));
      if (source) await this.startScan({ providerId: source.providerId, timezone: this.settings.timezone }, true);
    }
    this.emit({ kind: 'auto', status: await this.getAutoCollection() });
    this.scheduleCheck(this.autoConfig.intervalMinutes * 60000);
  }
  async getAutoCollection(): Promise<AutoCollectionStatus> {
    this.checkVersion();
    return { apiVersion: API_VERSION, revision: this.settings.revision, timezone: this.settings.timezone, config: structuredClone(this.autoConfig),
      providers: this.autoConfig.enabled ? this.settings.providers.filter(p => p.enabled).map(provider => {
        const latest = [...this.jobs.values()].reverse().find(job => job.automatic && job.summary.providerId === provider.providerId);
        const scan = latest ? this.readJob(latest.summary.jobId) : undefined;
        return { providerId: provider.providerId, state: scan && isActiveScan(scan) ? 'scanning' : scan?.state === 'failed' ? 'backoff' : this.baselines.has(provider.providerId) ? 'idle' : 'waiting',
          jobId: scan && isActiveScan(scan) ? scan.jobId : null, watching: true, error: scan?.error ?? null,
          lastSuccessAt: [...this.jobs.values()].reverse().find(job => job.summary.providerId === provider.providerId && job.summary.state === 'succeeded')?.summary.finishedAt ?? null,
          nextCheckAt: new Date(Math.max(Date.now(), this.eligibleAt.get(provider.providerId) ?? this.autoDeadline)).toISOString() };
      }) : [] };
  }
  async updateAutoCollection(request: UpdateAutoCollectionRequest): Promise<AutoCollectionStatus> {
    this.checkVersion();
    if (![1, 5, 15].includes(request.config.intervalMinutes) || typeof request.config.enabled !== 'boolean') throw apiError('INVALID_QUERY', '自动采集间隔无效。');
    if (request.expectedRevision !== this.settings.revision) throw apiError('SETTINGS_CONFLICT', '设置已变化，请重新读取。');
    const wasEnabled = this.autoConfig.enabled;
    this.autoConfig = structuredClone(request.config); this.settings.revision = (BigInt(this.settings.revision) + 1n).toString();
    if (this.autoTimer !== undefined) { clearTimeout(this.autoTimer); this.autoTimer = undefined; }
    if (!this.autoConfig.enabled) {
      for (const job of this.jobs.values()) if (job.automatic && isActiveScan(this.readJob(job.summary.jobId))) await this.cancelScan(job.summary.jobId);
      this.baselines.clear();
    } else { if (!wasEnabled) { this.baselines.clear(); this.eligibleAt.clear(); } this.scheduleCheck(); }
    this.tick(); // shared revision changed
    const status = await this.getAutoCollection(); this.emit({ kind: 'auto', status }); return status;
  }
  private timezoneStatus(sequence = '0'): TimezoneStatus {
    return { apiVersion: API_VERSION, sequence, revision: this.settings.revision, mode: this.timezoneMode, effectiveTimezone: this.settings.timezone,
      systemTimezone: this.systemZone, detectionError: structuredClone(this.detectionError), pendingTimezone: this.pendingZone, rebuild: this.rebuild,
      nextRetryAt: this.retryAt === null ? null : new Date(this.retryAt).toISOString(), providers: structuredClone(this.rebuildProviders) };
  }
  /** Every computed status (response or event) gets a strictly larger sequence. */
  private nextTimezoneStatus(): TimezoneStatus { this.timezoneSequence += 1n; return this.timezoneStatus(this.timezoneSequence.toString()); }
  private bumpRevision() { this.settings.revision = (BigInt(this.settings.revision) + 1n).toString(); }
  private clearRetry() { if (this.retryTimer !== undefined) { clearTimeout(this.retryTimer); this.retryTimer = undefined; } }
  private armRetry() {
    this.clearRetry();
    if (this.retryAt !== null) this.retryTimer = setTimeout(() => { this.retryTimer = undefined; this.tick(); }, Math.max(0, this.retryAt - Date.now()));
  }
  /** One synthetic rebuild entry per enabled source; disabled sources are never read. */
  private resetRebuild() {
    this.rebuildProviders = this.settings.providers.filter(provider => provider.enabled).map(provider => ({ providerId: provider.providerId, state: 'pending', jobId: null, error: null }));
    this.rebuild = this.rebuildProviders.length ? 'pending' : 'idle'; this.retryAt = null; this.clearRetry();
  }
  /** Reconciles detection, pending changes and the serial rebuild. Runs lazily on reads and from scan/retry timers. */
  private tick() {
    const busy = this.scanActive();
    for (const provider of this.rebuildProviders) {
      const scan = provider.state === 'rebuilding' && provider.jobId ? this.jobs.get(provider.jobId)?.summary : undefined;
      if (!scan || isActiveScan(scan)) continue;
      provider.jobId = null; provider.state = scan.state === 'succeeded' ? 'succeeded' : 'failed';
      provider.error = scan.state === 'succeeded' ? null : scan.error ?? apiError('CANCELLED', '时区重建已取消；已有历史保留，稍后自动重试。', true);
    }
    if (this.rebuild !== 'backoff' && this.rebuildProviders.some(provider => provider.state === 'failed')) { this.rebuild = 'backoff'; this.retryAt = Date.now() + REBUILD_RETRY_MS; this.armRetry(); }
    if (this.rebuild === 'backoff' && this.retryAt !== null && Date.now() >= this.retryAt) {
      this.rebuild = 'pending'; this.retryAt = null; this.clearRetry();
      for (const provider of this.rebuildProviders) if (provider.state === 'failed') { provider.state = 'pending'; provider.error = null; }
    }
    // Follow-system changes apply only while no scan is active; rapid changes coalesce to the latest detection.
    const target = this.timezoneMode === 'follow-system' && this.systemZone !== this.settings.timezone ? this.systemZone : null;
    this.pendingZone = target !== null && busy ? target : null;
    if (target !== null && !busy) { this.settings.timezone = target; this.bumpRevision(); this.resetRebuild(); }
    if (this.rebuild !== 'idle') {
      const enabled = this.settings.providers.filter(provider => provider.enabled);
      this.rebuildProviders = enabled.map(({ providerId }) => this.rebuildProviders.find(provider => provider.providerId === providerId) ?? { providerId, state: 'pending', jobId: null, error: null });
      const next = busy || this.rebuild === 'backoff' ? undefined : this.rebuildProviders.find(provider => provider.state === 'pending');
      if (next) { next.jobId = this.createJob(next.providerId, this.settings.timezone, false); next.state = 'rebuilding'; }
      if (this.rebuild !== 'backoff' || !this.rebuildProviders.length) {
        this.rebuild = this.rebuildProviders.some(provider => provider.state === 'rebuilding') ? 'rebuilding' : this.rebuildProviders.some(provider => provider.state !== 'succeeded') ? 'pending' : 'idle';
      }
      if (this.rebuild === 'idle') { this.rebuildProviders = []; this.retryAt = null; this.clearRetry(); }
    }
    const signature = JSON.stringify(this.timezoneStatus());
    if (signature !== this.timezoneSignature) { this.timezoneSignature = signature; this.emit({ kind: 'timezone', status: this.nextTimezoneStatus() }); }
  }
  async getTimezone(): Promise<TimezoneStatus> { this.checkVersion(); await delay(this.latencyMs); this.tick(); return this.nextTimezoneStatus(); }
  async updateTimezone(request: UpdateTimezoneRequest): Promise<TimezoneStatus> {
    this.checkVersion(); await delay(this.latencyMs);
    const zone = request.mode === 'fixed' && typeof request.timezone === 'string' ? canonicalZone(request.timezone) : null;
    if (request.mode === 'fixed' ? zone === null : request.mode !== 'follow-system' || request.timezone !== null) throw apiError('INVALID_QUERY', '时区设置无效：固定时区需要有效的 IANA 时区，跟随系统时不能指定时区。');
    if (request.expectedRevision !== this.settings.revision) throw apiError('SETTINGS_CONFLICT', '设置已变化，请重新读取。');
    this.tick();
    if (zone !== null && zone !== this.settings.timezone) {
      this.checkTimezone(zone);
      if (this.scanActive()) throw apiError('SCAN_BUSY', '扫描期间不能更改统计时区，请等待扫描完成。', true);
    }
    if (request.mode !== this.timezoneMode) { this.timezoneMode = request.mode; this.bumpRevision(); }
    if (zone !== null && zone !== this.settings.timezone) { this.settings.timezone = zone; this.bumpRevision(); this.resetRebuild(); }
    this.tick();
    return this.nextTimezoneStatus();
  }
  /** Synthetic OS timezone change for browser/transport acceptance. Never reads the real system zone. */
  simulateSystemTimezone(zone: string) {
    const detected = canonicalZone(zone);
    if (detected === null) { this.simulateDetectionFailure(); return; }
    this.systemZone = detected; this.detectionError = null; this.tick();
  }
  /** Detection failure keeps the effective zone (never silently UTC) and reports the error. */
  simulateDetectionFailure() { this.detectionError = apiError('INTERNAL', '无法检测系统时区；继续使用当前统计时区，稍后自动重试。', true); this.tick(); }
  async subscribeUsage(listener: (event: UsageEvent) => void): Promise<() => void> {
    this.listeners.add(listener); listener({ kind: 'resync' }); this.scheduleCheck();
    for (const [id, job] of this.jobs) if (isActiveScan(job.summary) && !this.completions.has(id)) {
      this.completions.set(id, setTimeout(() => { this.completions.delete(id); this.readJob(id); this.tick(); }, Math.max(0, this.scanMs - (Date.now() - job.clock))));
    }
    if (this.rebuild === 'backoff' && this.retryTimer === undefined) this.armRetry();
    return () => { this.listeners.delete(listener); if (this.listeners.size === 0) {
      if (this.autoTimer !== undefined) { clearTimeout(this.autoTimer); this.autoTimer = undefined; }
      for (const timer of this.completions.values()) clearTimeout(timer); this.completions.clear(); this.clearRetry();
    } };
  }
}
