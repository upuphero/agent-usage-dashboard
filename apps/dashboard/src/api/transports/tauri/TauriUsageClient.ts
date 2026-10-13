import { invoke } from '@tauri-apps/api/core';
import { listen } from '@tauri-apps/api/event';
import type { UsageClient, UsageEvent } from '../../client';
import {
  COMMANDS, type ApiInfo, type ProviderSummary, type StartScanRequest, type StartScanResult,
  type ScanSummary, type OverviewQuery, type OverviewResult, type SessionQuery, type SessionPage,
  type ExportRequest, type ExportResult, type SettingsResult, type UpdateSettingsRequest, type ChooseProviderDirectoryResult,
  SCAN_EVENT, AUTO_COLLECTION_EVENT, type AutoCollectionStatus, type UpdateAutoCollectionRequest,
  TIMEZONE_EVENT, type TimezoneStatus, type UpdateTimezoneRequest,
} from '../../generated/usage';
import { apiError, assertResponseVersion, normalizeError } from '../../protocol';
import { waitForScan } from '../scan';
import { isTauriRuntime } from './runtime';

export type CommandInvoker = <T>(command: string, args?: Record<string, unknown>) => Promise<T>;
export type EventSubscriber = (name: string, listener: (payload: unknown) => void) => Promise<() => void>;
const desktopListen: EventSubscriber = (name, listener) => listen(name, event => listener(event.payload));

const desktopInvoke: CommandInvoker = async (command, args) => {
  if (!isTauriRuntime()) {
    throw apiError('INTERNAL', '桌面连接不可用。请在桌面应用中打开，或切换到浏览器演示模式。');
  }
  return invoke(command, args);
};

export class TauriUsageClient implements UsageClient {
  private negotiation: Promise<ApiInfo> | undefined;

  constructor(private readonly send: CommandInvoker = desktopInvoke, private readonly pollMs = 750, private readonly events: EventSubscriber = desktopListen, private readonly resyncMs = 15000) {}

  private async command<T>(command: string, request?: unknown, language?: 'zh' | 'en'): Promise<T> {
    try {
      return await this.send<T>(command, request === undefined ? undefined : { request, ...(language ? { language } : {}) });
    } catch (reason) { throw normalizeError(reason); }
  }

  getApiInfo(): Promise<ApiInfo> {
    if (!this.negotiation) {
      this.negotiation = this.command<ApiInfo>(COMMANDS.getApiInfo).then(assertResponseVersion)
        .catch(reason => { this.negotiation = undefined; throw reason; });
    }
    return this.negotiation;
  }

  private async negotiated<T>(command: string, request?: unknown): Promise<T> {
    await this.getApiInfo();
    return this.command<T>(command, request);
  }

  listProviders(): Promise<ProviderSummary[]> { return this.negotiated(COMMANDS.listProviders); }
  startScan(request: StartScanRequest): Promise<StartScanResult> { return this.negotiated(COMMANDS.startScan, request); }
  getScan(jobId: string): Promise<ScanSummary> {
    return waitForScan(async () => assertResponseVersion(
      await this.negotiated<ScanSummary>(COMMANDS.getScan, { jobId }),
    ), this.pollMs);
  }
  cancelScan(jobId: string): Promise<void> { return this.negotiated(COMMANDS.cancelScan, { jobId }); }
  async getOverview(request: OverviewQuery): Promise<OverviewResult> {
    return assertResponseVersion(await this.negotiated<OverviewResult>(COMMANDS.getOverview, request));
  }
  async listSessions(request: SessionQuery): Promise<SessionPage> {
    return assertResponseVersion(await this.negotiated<SessionPage>(COMMANDS.listSessions, request));
  }
  async exportUsage(request: ExportRequest, language: 'zh' | 'en' = 'zh'): Promise<ExportResult> {
    const info = await this.getApiInfo();
    return assertResponseVersion(await this.command<ExportResult>(COMMANDS.exportUsage, request, info.capabilities.includes('localized-dialogs') ? language : undefined));
  }
  private async settingsCommand<T extends { apiVersion: string }>(capability: string, command: string, request?: unknown, language?: 'zh' | 'en'): Promise<T> {
    const info = await this.getApiInfo();
    if (!info.capabilities.includes(capability)) throw apiError('UNSUPPORTED_FILTER', '当前服务未提供这项设置能力。');
    return assertResponseVersion(await this.command<T>(command, request, info.capabilities.includes('localized-dialogs') ? language : undefined));
  }
  getSettings(): Promise<SettingsResult> { return this.settingsCommand('settings-read', COMMANDS.getSettings); }
  updateSettings(request: UpdateSettingsRequest): Promise<SettingsResult> { return this.settingsCommand('settings-write', COMMANDS.updateSettings, request); }
  chooseProviderDirectory(providerId: string, language: 'zh' | 'en' = 'zh'): Promise<ChooseProviderDirectoryResult> { return this.settingsCommand('source-directory-selection', COMMANDS.chooseProviderDirectory, { providerId }, language); }
  getAutoCollection(): Promise<AutoCollectionStatus> { return this.settingsCommand('auto-full-scan', COMMANDS.getAutoCollection); }
  updateAutoCollection(request: UpdateAutoCollectionRequest): Promise<AutoCollectionStatus> { return this.settingsCommand('auto-full-scan', COMMANDS.updateAutoCollection, request); }
  getTimezone(): Promise<TimezoneStatus> { return this.settingsCommand('timezone-follow-system', COMMANDS.getTimezone); }
  updateTimezone(request: UpdateTimezoneRequest): Promise<TimezoneStatus> { return this.settingsCommand('timezone-follow-system', COMMANDS.updateTimezone, request); }
  async subscribeUsage(listener: (event: UsageEvent) => void): Promise<() => void> {
    const info = await this.getApiInfo();
    let closed = false; let connected = false; let connecting = false;
    const removers: Array<() => void> = [];
    const connect = async () => {
      if (closed || connected || connecting || !info.capabilities.includes('scan-events')) return;
      connecting = true;
      const acquired: Array<() => void> = [];
      try {
        acquired.push(await this.events(SCAN_EVENT, payload => { if (!closed) listener({ kind: 'scan', scan: assertResponseVersion(payload as ScanSummary) }); }));
        if (info.capabilities.includes('auto-full-scan')) acquired.push(await this.events(AUTO_COLLECTION_EVENT, payload => { if (!closed) listener({ kind: 'auto', status: assertResponseVersion(payload as AutoCollectionStatus) }); }));
        // Older hosts never emit this event; only listen when the capability was negotiated.
        if (info.capabilities.includes('timezone-follow-system')) acquired.push(await this.events(TIMEZONE_EVENT, payload => { if (!closed) listener({ kind: 'timezone', status: assertResponseVersion(payload as TimezoneStatus) }); }));
        if (closed) acquired.forEach(remove => remove());
        else { removers.push(...acquired); connected = true; }
      } catch { acquired.forEach(remove => remove()); }
      finally { connecting = false; }
    };
    await connect();
    const resync = () => { if (!closed) { listener({ kind: 'resync' }); void connect(); } };
    const timer = setInterval(resync, this.resyncMs);
    const visible = () => { if (document.visibilityState === 'visible') resync(); };
    if (typeof window !== 'undefined') window.addEventListener('focus', resync);
    if (typeof document !== 'undefined') document.addEventListener('visibilitychange', visible);
    resync();
    return () => { closed = true; clearInterval(timer); removers.splice(0).forEach(remove => remove());
      if (typeof window !== 'undefined') window.removeEventListener('focus', resync);
      if (typeof document !== 'undefined') document.removeEventListener('visibilitychange', visible);
    };
  }
}
