import { afterEach, describe, expect, it, vi } from 'vitest';
import { MockUsageClient } from './mock/MockUsageClient';
import { TauriUsageClient, type CommandInvoker, type EventSubscriber } from './tauri/TauriUsageClient';
import { API_VERSION, AUTO_COLLECTION_EVENT, COMMANDS, SCAN_EVENT, type ApiInfo, type ScanSummary } from '../generated/usage';
import { DEMO_API_INFO, DEMO_TIMEZONE } from './mock/fixtures';
import type { UsageEvent } from '../client';

afterEach(() => { vi.useRealTimers(); vi.unstubAllGlobals(); });
const config = (enabled: boolean, intervalMinutes = 1) => ({ enabled, intervalMinutes });
async function enable(client: MockUsageClient) {
  const current = await client.getAutoCollection();
  return client.updateAutoCollection({ expectedRevision: current.revision, config: config(true) });
}
async function isolate(client: MockUsageClient) {
  const settings = await client.getSettings();
  await client.updateSettings({ expectedRevision: settings.revision, timezone: DEMO_TIMEZONE,
    providers: settings.providers.map((p, i) => ({ providerId: p.providerId, enabled: i === 0, directoryRef: null })) });
  return settings.providers[0].providerId;
}
describe('automatic full scan synthetic transport acceptance', () => {
  it('defaults off, validates intervals and checks the shared settings revision', async () => {
    const client = new MockUsageClient('partial', 0);
    expect((await client.getAutoCollection()).config).toEqual(config(false, 5));
    await expect(client.updateAutoCollection({ expectedRevision: '1', config: config(true, 2) })).rejects.toMatchObject({ code: 'INVALID_QUERY' });
    const result = await client.updateAutoCollection({ expectedRevision: '1', config: config(false, 15) });
    expect(result.config.intervalMinutes).toBe(15);
    await expect(client.updateAutoCollection({ expectedRevision: '1', config: config(false) })).rejects.toMatchObject({ code: 'SETTINGS_CONFLICT' });
    expect((await client.getSettings()).timezone).toBe(DEMO_TIMEZONE);
  });
  it('serializes initial scans, merges changes, skips clean inputs and retains writes during a job', async () => {
    const client = new MockUsageClient('partial', 0, 1000); const source = await isolate(client);
    vi.useFakeTimers(); const events: UsageEvent[] = []; const remove = await client.subscribeUsage(e => events.push(e));
    await enable(client); await vi.advanceTimersByTimeAsync(2000);
    const first = (await client.getAutoCollection()).providers[0].jobId!; expect(first).toBeTruthy();
    for (let i = 0; i < 100; i++) client.simulateSourceChange(source);
    await vi.advanceTimersByTimeAsync(1000); expect((await client.getAutoCollection()).providers[0].lastSuccessAt).toBeTruthy();
    await vi.advanceTimersByTimeAsync(5000); expect(events.filter(e => e.kind === 'scan' && e.scan.state === 'queued')).toHaveLength(1);
    await vi.advanceTimersByTimeAsync(60000); expect(events.filter(e => e.kind === 'scan' && e.scan.state === 'queued')).toHaveLength(2);
    await vi.advanceTimersByTimeAsync(120000); expect(events.filter(e => e.kind === 'scan' && e.scan.state === 'queued')).toHaveLength(2);
    expect((await client.getScan(first)).state).toBe('succeeded'); remove(); expect(vi.getTimerCount()).toBe(0);
  });
  it('turning auto off cancels its job while a manual job continues and keeps the source gate', async () => {
    const client = new MockUsageClient('partial', 0, 1000); const source = await isolate(client);
    vi.useFakeTimers(); const remove = await client.subscribeUsage(() => {}); await enable(client);
    await vi.advanceTimersByTimeAsync(2000); const automatic = (await client.getAutoCollection()).providers[0].jobId!;
    await client.updateAutoCollection({ expectedRevision: (await client.getAutoCollection()).revision, config: config(false) });
    expect((await client.getScan(automatic)).state).toBe('cancelled');
    const manual = await client.startScan({ providerId: source, timezone: DEMO_TIMEZONE });
    await client.updateAutoCollection({ expectedRevision: (await client.getAutoCollection()).revision, config: config(false, 15) });
    const saved = await client.getSettings();
    await expect(client.updateSettings({ expectedRevision: saved.revision, timezone: DEMO_TIMEZONE, providers: [] })).rejects.toMatchObject({ code: 'SCAN_BUSY' });
    await vi.advanceTimersByTimeAsync(1000); expect((await client.getScan(manual.jobId)).state).toBe('succeeded'); remove();
  });
  it('failure and cancellation preserve existing synthetic totals and wait before retrying', async () => {
    const client = new MockUsageClient('scan-error', 0, 1000); await isolate(client);
    const query = { range: { start: '2026-09-28', end: '2026-10-05' }, timezone: DEMO_TIMEZONE, bucket: 'day' as const, providerIds: [], modelIds: [] };
    const before = await client.getOverview(query);
    vi.useFakeTimers(); const events: UsageEvent[] = []; const remove = await client.subscribeUsage(e => events.push(e));
    await enable(client); await vi.advanceTimersByTimeAsync(3000);
    expect((await client.getAutoCollection()).providers[0].state).toBe('backoff');
    await vi.advanceTimersByTimeAsync(10000); expect(events.filter(e => e.kind === 'scan' && e.scan.state === 'queued')).toHaveLength(1);
    expect((await client.getOverview(query)).usage).toEqual(before.usage); remove();
  });
  it('older services reject auto calls without sending new commands or parameters', async () => {
    const sent: string[] = []; const send: CommandInvoker = async <T,>(name: string) => {
      sent.push(name); return { ...DEMO_API_INFO, apiVersion: '1.1.0', capabilities: ['settings-read'] } as T;
    };
    const client = new TauriUsageClient(send);
    await expect(client.updateAutoCollection({ expectedRevision: '1', config: config(true) })).rejects.toMatchObject({ code: 'UNSUPPORTED_FILTER' });
    expect(sent).toEqual([COMMANDS.getApiInfo]);
  });
  it('subscribes once per mount, delivers unseen scan results, recovers focus and removes timers/listeners', async () => {
    vi.useFakeTimers();
    const payloads = new Map<string, (payload: unknown) => void>(); const removed = vi.fn();
    const listen: EventSubscriber = async (name, callback) => { payloads.set(name, callback); return () => { payloads.delete(name); removed(); }; };
    const send: CommandInvoker = async <T,>() => DEMO_API_INFO as ApiInfo as T;
    const focus = new Map<string, () => void>();
    vi.stubGlobal('window', { addEventListener: (name: string, callback: () => void) => focus.set(name, callback), removeEventListener: (name: string) => focus.delete(name) });
    const client = new TauriUsageClient(send, 1, listen, 1000); const events: UsageEvent[] = [];
    let remove = await client.subscribeUsage(e => events.push(e)); expect(payloads.size).toBe(2);
    const scan: ScanSummary = { apiVersion: API_VERSION, jobId: 'background', providerId: 'fixture', state: 'succeeded', startedAt: '2026-10-05T10:00:00Z', finishedAt: '2026-10-05T10:00:01Z', error: null, rowsWritten: 1, snapshotsReplaced: 2 };
    payloads.get(SCAN_EVENT)!(scan); expect(events).toContainEqual({ kind: 'scan', scan });
    expect(payloads.has(AUTO_COLLECTION_EVENT)).toBe(true); focus.get('focus')!();
    await vi.advanceTimersByTimeAsync(1000); expect(events.filter(e => e.kind === 'resync')).toHaveLength(3);
    remove(); expect(vi.getTimerCount()).toBe(0); expect(payloads.size).toBe(0); expect(focus.size).toBe(0);
    remove = await client.subscribeUsage(() => {}); remove(); expect(removed).toHaveBeenCalledTimes(4);
  });
  it('retries failed event attachment with polling and releases partial registrations', async () => {
    vi.useFakeTimers(); let attempts = 0; const remove = vi.fn();
    const subscribe: EventSubscriber = async name => { if (name === AUTO_COLLECTION_EVENT && attempts++ === 0) throw Error('unavailable'); return remove; };
    const send: CommandInvoker = async <T,>() => DEMO_API_INFO as T;
    const events: UsageEvent[] = []; const stop = await new TauriUsageClient(send, 1, subscribe, 1000).subscribeUsage(e => events.push(e));
    await vi.advanceTimersByTimeAsync(1000); expect(attempts).toBe(2); expect(events.some(e => e.kind === 'resync')).toBe(true); stop(); expect(remove).toHaveBeenCalledTimes(3);
  });
});
