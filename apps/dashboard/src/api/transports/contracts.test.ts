import { describe, expect, it } from 'vitest';
import type { UsageClient } from '../client';
import { API_VERSION, COMMANDS, type OverviewQuery, type ScanSummary } from '../generated/usage';
import { apiError } from '../protocol';
import { MockUsageClient } from './mock/MockUsageClient';
import { ALL_USAGE, DEMO_API_INFO, DEMO_RANGE, DEMO_TIMEZONE, PROVIDERS, SESSIONS } from './mock/fixtures';
import { TauriUsageClient, type CommandInvoker } from './tauri/TauriUsageClient';
import { waitForScan } from './scan';

const query: OverviewQuery = { range: DEMO_RANGE, timezone: DEMO_TIMEZONE, providerIds: [], modelIds: [], bucket: 'day' };
const terminal: ScanSummary = { apiVersion: API_VERSION, jobId: 'wire-job', providerId: PROVIDERS[0].providerId, state: 'succeeded', startedAt: '2026-10-04T18:40:00Z', finishedAt: '2026-10-04T18:42:00Z', error: null, snapshotsReplaced: 2, rowsWritten: 9 };

function wireClient() {
  const calls: Array<{ command: string; args?: Record<string, unknown> }> = [];
  const send: CommandInvoker = async <T,>(command: string, args?: Record<string, unknown>) => {
    calls.push({ command, args });
    const request = args?.request;
    let result: unknown;
    switch (command) {
      case COMMANDS.getApiInfo: result = DEMO_API_INFO; break;
      case COMMANDS.listProviders: result = PROVIDERS; break;
      case COMMANDS.startScan: result = { jobId: terminal.jobId }; break;
      case COMMANDS.getScan: result = terminal; break;
      case COMMANDS.cancelScan: result = undefined; break;
      case COMMANDS.getOverview: result = { apiVersion: API_VERSION, query: request, usage: ALL_USAGE, byProvider: [], byModel: [], buckets: [], coverage: [], warnings: [], stale: false, lastSuccessAt: null }; break;
      case COMMANDS.listSessions: result = { apiVersion: API_VERSION, items: SESSIONS, total: 2, nextOffset: null, stale: false, warnings: [], dateFilterSemantics: 'active-sessions-lifetime-usage' }; break;
      case COMMANDS.exportUsage: result = { apiVersion: API_VERSION, exportId: 'opaque', suggestedFilename: 'usage.csv', mediaType: 'text/csv', byteLength: '12' }; break;
      default: throw new Error(`Unexpected command ${command}`);
    }
    return structuredClone(result) as T;
  };
  return { client: new TauriUsageClient(send, 0), calls };
}

describe.each(['Mock', 'Tauri'] as const)('%s consumes the generated UsageClient contract', transport => {
  const create = (): UsageClient => transport === 'Mock' ? new MockUsageClient('partial', 0, 0) : wireClient().client;
  it('negotiates v1 and preserves decimal strings, null, zero and field accuracy', async () => {
    const client = create();
    expect((await client.getApiInfo()).apiVersion).toBe(API_VERSION);
    const result = await client.getOverview(query);
    expect(result.query).toEqual(query);
    expect(result.usage.tokens.total.value).toBe('1480000');
    expect(result.usage.tokens.outputReasoning).toMatchObject({ value: null, accuracy: 'unavailable' });
    expect(result.usage.tokens.cacheWrite.value).toBe('0');
    expect(result.usage.cost).toMatchObject({ kind: 'api-equivalent-estimate', amountUsd: { value: '12.46000000', accuracy: 'estimated', missingRows: 1 } });
  });
  it('consumes descriptors, session lifetime semantics, scan terminals and opaque exports', async () => {
    const client = create();
    expect((await client.listProviders())[0].capabilities.supportsDateSessionIntersection).toBe(false);
    const sessions = await client.listSessions({ timezone: DEMO_TIMEZONE, providerIds: [], modelIds: [], activeRange: null, offset: 0, limit: 20 });
    expect(sessions.dateFilterSemantics).toBe('active-sessions-lifetime-usage');
    expect(sessions.items[0].startedAt).toBeNull();
    expect(sessions.nextOffset).toBeNull();
    const { jobId } = await client.startScan({ providerId: PROVIDERS[0].providerId, timezone: DEMO_TIMEZONE });
    expect((await client.getScan(jobId)).state).toBe('succeeded');
    await expect(client.cancelScan(jobId)).resolves.toBeUndefined();
    const exported = await client.exportUsage({ format: 'csv', query });
    expect(exported).not.toHaveProperty('path');
    expect(typeof exported.byteLength).toBe('string');
  });
});

describe('Tauri transport boundary', () => {
  it('uses generated command names and exact { request } envelopes', async () => {
    const { client, calls } = wireClient();
    await client.listProviders(); await client.getOverview(query);
    const scan = { providerId: PROVIDERS[0].providerId, timezone: DEMO_TIMEZONE };
    await client.startScan(scan); await client.getScan('wire-job'); await client.cancelScan('wire-job');
    expect(calls[0]).toEqual({ command: COMMANDS.getApiInfo, args: undefined });
    expect(calls[1]).toEqual({ command: COMMANDS.listProviders, args: undefined });
    expect(calls).toContainEqual({ command: COMMANDS.getOverview, args: { request: query } });
    expect(calls).toContainEqual({ command: COMMANDS.startScan, args: { request: scan } });
    expect(calls).toContainEqual({ command: COMMANDS.getScan, args: { request: { jobId: 'wire-job' } } });
    expect(calls).toContainEqual({ command: COMMANDS.cancelScan, args: { request: { jobId: 'wire-job' } } });
    expect(calls.filter(call => call.command === COMMANDS.getApiInfo)).toHaveLength(1);
  });
  it('blocks all business queries on incompatible API versions', async () => {
    const calls: string[] = [];
    const send: CommandInvoker = async <T,>(command: string) => { calls.push(command); return { ...DEMO_API_INFO, apiVersion: '2.0.0' } as T; };
    const client = new TauriUsageClient(send);
    await expect(client.listProviders()).rejects.toMatchObject({ code: 'API_VERSION_UNSUPPORTED' });
    expect(calls).toEqual([COMMANDS.getApiInfo]);
  });
  it('preserves structured IPC errors and rejects unavailable browser bridges explicitly', async () => {
    const send: CommandInvoker = async <T,>(command: string) => {
      if (command === COMMANDS.getApiInfo) return DEMO_API_INFO as T;
      throw JSON.stringify(apiError('SCHEMA_UNSUPPORTED', '格式不支持'));
    };
    await expect(new TauriUsageClient(send).listProviders()).rejects.toMatchObject({ code: 'SCHEMA_UNSUPPORTED', retryable: false });
    await expect(new TauriUsageClient().getApiInfo()).rejects.toMatchObject({ code: 'INTERNAL', retryable: false });
  });
  it('retains u64 strings without numeric coercion', async () => {
    const fixture = await new MockUsageClient('partial', 0).getOverview(query);
    fixture.usage.tokens.total.value = '18446744073709551615';
    const send: CommandInvoker = async <T,>(command: string) => (command === COMMANDS.getApiInfo ? DEMO_API_INFO : fixture) as T;
    expect((await new TauriUsageClient(send).getOverview(query)).usage.tokens.total.value).toBe('18446744073709551615');
  });
  it('polls queued/running states inside the transport and preserves failed terminal details', async () => {
    const scans: ScanSummary[] = [{ ...terminal, state: 'queued' }, { ...terminal, state: 'running' }, { ...terminal, state: 'failed', error: apiError('TIMEOUT', '采集超时', true) }];
    let reads = 0;
    const send: CommandInvoker = async <T,>(command: string) => (command === COMMANDS.getApiInfo ? DEMO_API_INFO : scans[reads++]) as T;
    const result = await new TauriUsageClient(send, 0).getScan('wire-job');
    expect(reads).toBe(3);
    expect(result).toMatchObject({ state: 'failed', error: { code: 'TIMEOUT' } });
  });
  it('bounds scan waiting without pretending the job was cancelled', async () => {
    await expect(waitForScan(async () => ({ ...terminal, state: 'running' }), 0, 2)).rejects.toMatchObject({ code: 'TIMEOUT', retryable: true });
  });
});

describe('Mock scenarios are explicit, repeatable fixture results', () => {
  it('keeps stale/partial coverage and unknown cost separate from zero', async () => {
    const result = await new MockUsageClient('stale', 0).getOverview(query);
    expect(result.stale).toBe(true);
    expect(result.warnings).toContain('DEMO_DATA');
    expect(result.coverage.some(value => value.state === 'partial')).toBe(true);
    const cost = result.byProvider.find(value => value.id === 'ccusage.antigravity')!.usage.cost.amountUsd;
    expect(cost.value).toBeNull();
  });
  it('returns a known zero only for a successful empty fixture', async () => {
    const result = await new MockUsageClient('empty', 0).getOverview(query);
    expect(result.usage.tokens.total).toMatchObject({ value: '0', accuracy: 'derived', knownRows: 0 });
    expect(result.usage.tokens.inputUncached.value).toBeNull();
    expect(result.byProvider).toEqual([]);
    expect(result.coverage.every(coverage => coverage.state === 'complete')).toBe(true);
    expect(result.lastSuccessAt).not.toBeNull();
  });
  it('uses pre-authored date/source/model results and rejects unsupported cross filters', async () => {
    const client = new MockUsageClient('partial', 0);
    expect((await client.getOverview({ ...query, range: { start: '2026-10-04', end: '2026-10-05' } })).usage.tokens.total.value).toBe('135000');
    expect((await client.getOverview({ ...query, providerIds: ['ccusage.codex'], modelIds: ['demo-codex-model'] })).usage.tokens.total.value).toBe('560000');
    await expect(client.getOverview({ ...query, modelIds: ['demo-codex-model'] })).rejects.toMatchObject({ code: 'UNSUPPORTED_FILTER' });
    await expect(new MockUsageClient('limited', 0).listSessions({ timezone: DEMO_TIMEZONE, providerIds: [], modelIds: [], activeRange: null, offset: 0, limit: 20 })).rejects.toMatchObject({ code: 'UNSUPPORTED_FILTER' });
  });
  it('paginates session lifetime data and does not reattribute totals by activity date', async () => {
    const client = new MockUsageClient('partial', 0);
    const base = { timezone: DEMO_TIMEZONE, providerIds: [], modelIds: [], activeRange: null, offset: 0, limit: 1 };
    const page = await client.listSessions(base);
    expect(page.nextOffset).toBe(1);
    expect(page.items[0].usage.tokens.total.value).toBe('820000');
    expect((await client.listSessions({ ...base, offset: 1 })).items[0].sessionId).toBe('demo-session-02');
    await expect(client.listSessions({ ...base, limit: 0 })).rejects.toMatchObject({ code: 'INVALID_QUERY' });
  });
  it('merges duplicate scans, rejects a concurrent timezone change, and cancels idempotently', async () => {
    const client = new MockUsageClient('partial', 0, 1000);
    const request = { providerId: PROVIDERS[0].providerId, timezone: DEMO_TIMEZONE };
    const first = await client.startScan(request);
    expect(await client.startScan(request)).toEqual(first);
    await expect(client.startScan({ ...request, timezone: 'UTC' })).rejects.toMatchObject({ code: 'SCAN_BUSY' });
    await client.cancelScan(first.jobId); await client.cancelScan(first.jobId);
    expect((await client.getScan(first.jobId)).state).toBe('cancelled');
  });
  it('retains the same snapshot on scan failure and rejects filtered JSON archives', async () => {
    const client = new MockUsageClient('scan-error', 0, 0);
    const before = await client.getOverview(query);
    const { jobId } = await client.startScan({ providerId: PROVIDERS[0].providerId, timezone: DEMO_TIMEZONE });
    expect((await client.getScan(jobId)).state).toBe('failed');
    const after = await client.getOverview(query);
    expect(after.usage).toEqual(before.usage);
    expect(after.stale).toBe(true);
    await expect(client.exportUsage({ format: 'json', query: { ...query, modelIds: ['demo-codex-model'] } })).rejects.toMatchObject({ code: 'UNSUPPORTED_FILTER' });
  });
});
