import { describe, expect, it } from 'vitest';
import { API_VERSION, COMMANDS, type SettingsResult, type UpdateSettingsRequest } from '../generated/usage';
import { apiError } from '../protocol';
import { MockUsageClient } from './mock/MockUsageClient';
import { DEMO_API_INFO, DEMO_RANGE, DEMO_TIMEZONE, PROVIDERS } from './mock/fixtures';
import { TauriUsageClient, type CommandInvoker } from './tauri/TauriUsageClient';

describe('API 1.1 settings', () => {
  it('Mock toggles collection without deleting or recomputing historical usage', async () => {
    const client = new MockUsageClient('partial', 0);
    const before = await client.getSettings();
    const result = await client.updateSettings({ expectedRevision: before.revision, timezone: before.timezone, providers: [{ providerId: PROVIDERS[0].providerId, enabled: false, directoryRef: null }] });
    expect(result.revision).toBe('2');
    expect((await client.listProviders())[0].enabled).toBe(false);
    await expect(client.startScan({ providerId: PROVIDERS[0].providerId, timezone: DEMO_TIMEZONE })).rejects.toMatchObject({ code: 'PROVIDER_DISABLED' });
    expect((await client.getOverview({ range: DEMO_RANGE, timezone: DEMO_TIMEZONE, providerIds: [], modelIds: [], bucket: 'day' })).usage.tokens.total.value).toBe('1480000');
    expect((await new MockUsageClient('partial', 0).getSettings()).revision).toBe('1');
  });
  it('Mock preserves opaque directory refs and rejects stale revisions and foreign paths', async () => {
    const client = new MockUsageClient('partial', 0);
    const selected = await client.chooseProviderDirectory(PROVIDERS[0].providerId);
    expect(selected.directory).not.toHaveProperty('path');
    const request: UpdateSettingsRequest = { expectedRevision: '1', timezone: DEMO_TIMEZONE, providers: [{ providerId: PROVIDERS[0].providerId, enabled: true, directoryRef: selected.directory!.directoryRef }] };
    expect((await client.updateSettings(request)).providers[0].directory?.directoryRef).toBe(selected.directory!.directoryRef);
    await expect(client.updateSettings(request)).rejects.toMatchObject({ code: 'SETTINGS_CONFLICT' });
    await expect(client.updateSettings({ ...request, expectedRevision: '2', providers: [{ ...request.providers[0], directoryRef: 'C:/private' }] })).rejects.toMatchObject({ code: 'INVALID_DIRECTORY_REF' });
    await expect(client.updateSettings({ ...request, expectedRevision: '2', providers: [{ ...request.providers[0], providerId: PROVIDERS[1].providerId }] })).rejects.toMatchObject({ code: 'INVALID_DIRECTORY_REF' });
    expect((await client.getSettings()).revision).toBe('2');
  });
  it('Mock rejects settings writes while a scan is active', async () => {
    const client = new MockUsageClient('partial', 0, 60_000);
    await client.startScan({ providerId: PROVIDERS[0].providerId, timezone: DEMO_TIMEZONE });
    await expect(client.updateSettings({ expectedRevision: '1', timezone: DEMO_TIMEZONE, providers: [] })).rejects.toMatchObject({ code: 'SCAN_BUSY' });
  });
  it('Tauri uses exact request envelopes and propagates generated settings errors', async () => {
    const result: SettingsResult = { apiVersion: API_VERSION, revision: '1', timezone: 'UTC', providers: [], collectionNotice: 'notice', directoryChangePolicy: 'preserve-dataset' };
    const calls: Array<{ command: string; args?: Record<string, unknown> }> = [];
    const send: CommandInvoker = async <T,>(command: string, args?: Record<string, unknown>) => {
      calls.push({ command, args });
      if (command === COMMANDS.getApiInfo) return DEMO_API_INFO as T;
      if (command === COMMANDS.updateSettings) throw JSON.stringify(apiError('SETTINGS_CONFLICT', '重新读取'));
      if (command === COMMANDS.chooseProviderDirectory) return { apiVersion: API_VERSION, providerId: 'provider', directory: null } as T;
      return result as T;
    };
    const client = new TauriUsageClient(send);
    await client.getSettings(); await client.chooseProviderDirectory('provider');
    const request = { expectedRevision: '1', timezone: 'UTC', providers: [] };
    await expect(client.updateSettings(request)).rejects.toMatchObject({ code: 'SETTINGS_CONFLICT' });
    expect(calls).toContainEqual({ command: COMMANDS.getSettings, args: undefined });
    expect(calls).toContainEqual({ command: COMMANDS.chooseProviderDirectory, args: { request: { providerId: 'provider' } } });
    expect(calls).toContainEqual({ command: COMMANDS.updateSettings, args: { request } });
  });
  it('older v1 services without capabilities never receive unknown settings commands', async () => {
    const calls: string[] = [];
    const send: CommandInvoker = async <T,>(command: string) => { calls.push(command); return { ...DEMO_API_INFO, apiVersion: '1.0.0', capabilities: ['overview'] } as T; };
    await expect(new TauriUsageClient(send).getSettings()).rejects.toMatchObject({ code: 'UNSUPPORTED_FILTER' });
    expect(calls).toEqual([COMMANDS.getApiInfo]);
  });
});
