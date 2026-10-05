import { invoke } from '@tauri-apps/api/core';
import type { UsageClient } from '../../client';
import {
  COMMANDS, type ApiInfo, type ProviderSummary, type StartScanRequest, type StartScanResult,
  type ScanSummary, type OverviewQuery, type OverviewResult, type SessionQuery, type SessionPage,
  type ExportRequest, type ExportResult, type SettingsResult, type UpdateSettingsRequest, type ChooseProviderDirectoryResult,
} from '../../generated/usage';
import { apiError, assertResponseVersion, normalizeError } from '../../protocol';
import { waitForScan } from '../scan';
import { isTauriRuntime } from './runtime';

export type CommandInvoker = <T>(command: string, args?: Record<string, unknown>) => Promise<T>;

const desktopInvoke: CommandInvoker = async (command, args) => {
  if (!isTauriRuntime()) {
    throw apiError('INTERNAL', '桌面连接不可用。请在桌面应用中打开，或切换到浏览器演示模式。');
  }
  return invoke(command, args);
};

export class TauriUsageClient implements UsageClient {
  private negotiation: Promise<ApiInfo> | undefined;

  constructor(private readonly send: CommandInvoker = desktopInvoke, private readonly pollMs = 750) {}

  private async command<T>(command: string, request?: unknown): Promise<T> {
    try {
      return await this.send<T>(command, request === undefined ? undefined : { request });
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
  async exportUsage(request: ExportRequest): Promise<ExportResult> {
    return assertResponseVersion(await this.negotiated<ExportResult>(COMMANDS.exportUsage, request));
  }
  private async settingsCommand<T extends { apiVersion: string }>(capability: string, command: string, request?: unknown): Promise<T> {
    const info = await this.getApiInfo();
    if (!info.capabilities.includes(capability)) throw apiError('UNSUPPORTED_FILTER', '当前服务未提供这项设置能力。');
    return assertResponseVersion(await this.command<T>(command, request));
  }
  getSettings(): Promise<SettingsResult> { return this.settingsCommand('settings-read', COMMANDS.getSettings); }
  updateSettings(request: UpdateSettingsRequest): Promise<SettingsResult> { return this.settingsCommand('settings-write', COMMANDS.updateSettings, request); }
  chooseProviderDirectory(providerId: string): Promise<ChooseProviderDirectoryResult> { return this.settingsCommand('source-directory-selection', COMMANDS.chooseProviderDirectory, { providerId }); }
}
