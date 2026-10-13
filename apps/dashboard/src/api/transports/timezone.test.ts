import { afterEach, describe, expect, it, vi } from 'vitest';
import { MockUsageClient } from './mock/MockUsageClient';
import { TauriUsageClient, type CommandInvoker, type EventSubscriber } from './tauri/TauriUsageClient';
import { API_VERSION, AUTO_COLLECTION_EVENT, COMMANDS, SCAN_EVENT, TIMEZONE_EVENT, type ApiInfo, type TimezoneStatus, type UpdateTimezoneRequest } from '../generated/usage';
import { DEMO_API_INFO, DEMO_RANGE, DEMO_TIMEZONE, PROVIDERS } from './mock/fixtures';
import type { UsageEvent } from '../client';

afterEach(() => { vi.useRealTimers(); vi.unstubAllGlobals(); });
const [first, second] = PROVIDERS.map(provider => provider.providerId);
const query = { range: DEMO_RANGE, timezone: DEMO_TIMEZONE, bucket: 'day' as const, providerIds: [], modelIds: [] };
const statuses = (events: UsageEvent[]) => events.flatMap(event => event.kind === 'timezone' ? [event.status] : []);
const queued = (events: UsageEvent[]) => events.flatMap(event => event.kind === 'scan' && event.scan.state === 'queued' ? [event.scan] : []);
const increasing = (values: TimezoneStatus[]) => values.every((value, index) => index === 0 || BigInt(value.sequence) > BigInt(values[index - 1].sequence));
async function enableOnly(client: MockUsageClient, enabled: string[]) {
  const settings = await client.getSettings();
  return client.updateSettings({ expectedRevision: settings.revision, timezone: settings.timezone, providers: settings.providers.map(provider => ({ providerId: provider.providerId, enabled: enabled.includes(provider.providerId), directoryRef: null })) });
}
const legacyInfo = { ...DEMO_API_INFO, apiVersion: '1.2.0', capabilities: DEMO_API_INFO.capabilities.filter(capability => capability !== 'timezone-follow-system') };
const wireStatus: TimezoneStatus = { apiVersion: API_VERSION, sequence: '7', revision: '4', mode: 'fixed', effectiveTimezone: 'Europe/Berlin', systemTimezone: null, detectionError: null, pendingTimezone: null, rebuild: 'idle', nextRetryAt: null, providers: [] };

describe('API 1.3 follow-system timezone synthetic transport acceptance', () => {
  it('defaults to following the demo system zone with strictly increasing sequences', async () => {
    const client = new MockUsageClient('partial', 0);
    const status = await client.getTimezone();
    expect(status).toMatchObject({ apiVersion: API_VERSION, revision: '1', mode: 'follow-system', effectiveTimezone: DEMO_TIMEZONE, systemTimezone: DEMO_TIMEZONE, detectionError: null, pendingTimezone: null, rebuild: 'idle', nextRetryAt: null, providers: [] });
    expect(BigInt((await client.getTimezone()).sequence)).toBeGreaterThan(BigInt(status.sequence));
    expect((await client.getSettings()).timezone).toBe(DEMO_TIMEZONE);
  });
  it('validates mode/zone pairs, the shared revision and the demo-zone rule before writing', async () => {
    const client = new MockUsageClient('partial', 0);
    const update = (request: Partial<UpdateTimezoneRequest>) => client.updateTimezone({ expectedRevision: '1', mode: 'fixed', timezone: DEMO_TIMEZONE, ...request });
    await expect(update({ timezone: null })).rejects.toMatchObject({ code: 'INVALID_QUERY' });
    await expect(update({ timezone: 'Not/AZone' })).rejects.toMatchObject({ code: 'INVALID_QUERY' });
    await expect(update({ mode: 'follow-system' })).rejects.toMatchObject({ code: 'INVALID_QUERY' });
    await expect(update({ expectedRevision: '0' })).rejects.toMatchObject({ code: 'SETTINGS_CONFLICT' });
    await expect(update({ timezone: 'Asia/Tokyo' })).rejects.toMatchObject({ code: 'UNSUPPORTED_FILTER' });
    expect((await client.getTimezone()).revision).toBe('1');
    // Aliases are canonicalized; the same zone changes only the mode and never rebuilds.
    expect(await update({ timezone: 'america/phoenix' })).toMatchObject({ mode: 'fixed', effectiveTimezone: DEMO_TIMEZONE, revision: '2', rebuild: 'idle', providers: [] });
    expect((await client.getSettings()).revision).toBe('2');
    await expect(client.updateAutoCollection({ expectedRevision: '1', config: { enabled: false, intervalMinutes: 5 } })).rejects.toMatchObject({ code: 'SETTINGS_CONFLICT' });
    expect(await client.updateTimezone({ expectedRevision: '2', mode: 'follow-system', timezone: null })).toMatchObject({ mode: 'follow-system', revision: '3', rebuild: 'idle' });
  });
  it('follows a detected change, bumps the revision and rebuilds only enabled sources serially back to idle', async () => {
    const client = new MockUsageClient('partial', 0, 1000); await enableOnly(client, [first, second]);
    vi.useFakeTimers(); const events: UsageEvent[] = []; const remove = await client.subscribeUsage(event => events.push(event));
    client.simulateSystemTimezone('Asia/Tokyo');
    const status = await client.getTimezone();
    expect(status).toMatchObject({ mode: 'follow-system', effectiveTimezone: 'Asia/Tokyo', systemTimezone: 'Asia/Tokyo', revision: '3', rebuild: 'rebuilding', pendingTimezone: null });
    expect(status.providers.map(provider => [provider.providerId, provider.state])).toEqual([[first, 'rebuilding'], [second, 'pending']]);
    expect(await client.getSettings()).toMatchObject({ timezone: 'Asia/Tokyo', revision: '3' });
    await expect(client.updateTimezone({ expectedRevision: '3', mode: 'fixed', timezone: DEMO_TIMEZONE })).rejects.toMatchObject({ code: 'SCAN_BUSY' });
    await vi.advanceTimersByTimeAsync(1000);
    expect((await client.getTimezone()).providers.map(provider => provider.state)).toEqual(['succeeded', 'rebuilding']);
    await vi.advanceTimersByTimeAsync(1000);
    expect(await client.getTimezone()).toMatchObject({ effectiveTimezone: 'Asia/Tokyo', revision: '3', rebuild: 'idle', providers: [] });
    expect(queued(events).map(scan => scan.providerId)).toEqual([first, second]);
    expect(statuses(events).map(value => value.rebuild)).toEqual(['rebuilding', 'rebuilding', 'idle']);
    expect(increasing(statuses(events))).toBe(true);
    // The demo keeps synthetic data for America/Phoenix only; it never fabricates other zones.
    await expect(client.getOverview({ ...query, timezone: 'Asia/Tokyo' })).rejects.toMatchObject({ code: 'UNSUPPORTED_FILTER' });
    remove(); expect(vi.getTimerCount()).toBe(0);
  });
  it('fixed mode ignores system changes; switching back to follow-system waits for the active scan', async () => {
    const client = new MockUsageClient('partial', 0, 1000); await enableOnly(client, [first]);
    await client.updateTimezone({ expectedRevision: '2', mode: 'fixed', timezone: DEMO_TIMEZONE });
    vi.useFakeTimers(); const events: UsageEvent[] = []; const remove = await client.subscribeUsage(event => events.push(event));
    client.simulateSystemTimezone('Europe/Berlin');
    expect(await client.getTimezone()).toMatchObject({ mode: 'fixed', effectiveTimezone: DEMO_TIMEZONE, systemTimezone: 'Europe/Berlin', pendingTimezone: null, rebuild: 'idle', revision: '3' });
    expect(queued(events)).toHaveLength(0);
    await client.startScan({ providerId: first, timezone: DEMO_TIMEZONE });
    expect(await client.updateTimezone({ expectedRevision: '3', mode: 'follow-system', timezone: null })).toMatchObject({ mode: 'follow-system', effectiveTimezone: DEMO_TIMEZONE, pendingTimezone: 'Europe/Berlin', revision: '4' });
    await vi.advanceTimersByTimeAsync(1000);
    expect(await client.getTimezone()).toMatchObject({ effectiveTimezone: 'Europe/Berlin', pendingTimezone: null, revision: '5', rebuild: 'rebuilding' });
    await vi.advanceTimersByTimeAsync(1000); expect((await client.getTimezone()).rebuild).toBe('idle');
    remove(); expect(vi.getTimerCount()).toBe(0);
  });
  it('detection failure keeps the effective zone and reports the error until detection succeeds again', async () => {
    const client = new MockUsageClient('partial', 0);
    client.simulateDetectionFailure();
    const failed = await client.getTimezone();
    expect(failed).toMatchObject({ mode: 'follow-system', effectiveTimezone: DEMO_TIMEZONE, systemTimezone: DEMO_TIMEZONE, revision: '1', rebuild: 'idle', detectionError: { code: 'INTERNAL', retryable: true } });
    expect(failed.effectiveTimezone).not.toBe('UTC');
    client.simulateSystemTimezone(DEMO_TIMEZONE);
    expect(await client.getTimezone()).toMatchObject({ detectionError: null, effectiveTimezone: DEMO_TIMEZONE, revision: '1' });
  });
  it('coalesces rapid changes during an active scan and applies only the latest after it finishes', async () => {
    const client = new MockUsageClient('partial', 0, 1000);
    vi.useFakeTimers(); const events: UsageEvent[] = []; const remove = await client.subscribeUsage(event => events.push(event));
    const { jobId } = await client.startScan({ providerId: first, timezone: DEMO_TIMEZONE });
    for (const zone of ['Europe/Berlin', 'Asia/Tokyo', 'Asia/Seoul']) client.simulateSystemTimezone(zone);
    expect(await client.getTimezone()).toMatchObject({ effectiveTimezone: DEMO_TIMEZONE, pendingTimezone: 'Asia/Seoul', revision: '1', rebuild: 'idle' });
    client.simulateSystemTimezone(DEMO_TIMEZONE); expect((await client.getTimezone()).pendingTimezone).toBeNull();
    client.simulateSystemTimezone('Asia/Tokyo'); client.simulateSystemTimezone('Asia/Seoul');
    await vi.advanceTimersByTimeAsync(1000);
    expect((await client.getScan(jobId)).state).toBe('succeeded');
    expect(await client.getTimezone()).toMatchObject({ effectiveTimezone: 'Asia/Seoul', pendingTimezone: null, revision: '2', rebuild: 'rebuilding' });
    await vi.advanceTimersByTimeAsync(3000);
    expect((await client.getTimezone()).rebuild).toBe('idle');
    expect(queued(events).slice(1).map(scan => scan.providerId)).toEqual(PROVIDERS.map(provider => provider.providerId));
    expect(statuses(events).map(value => value.effectiveTimezone)).not.toContain('Asia/Tokyo');
    expect(increasing(statuses(events))).toBe(true);
    remove(); expect(vi.getTimerCount()).toBe(0);
  });
  it('failed rebuilds back off with history and synthetic totals retained, then retry; timers stop on unsubscribe', async () => {
    const client = new MockUsageClient('scan-error', 0, 1000); await enableOnly(client, [first]);
    const before = await client.getOverview(query);
    vi.useFakeTimers(); const events: UsageEvent[] = []; const remove = await client.subscribeUsage(event => events.push(event));
    client.simulateSystemTimezone('Asia/Tokyo');
    await vi.advanceTimersByTimeAsync(1000);
    const status = await client.getTimezone();
    expect(status).toMatchObject({ effectiveTimezone: 'Asia/Tokyo', rebuild: 'backoff', providers: [{ providerId: first, state: 'failed', jobId: null, error: { code: 'COLLECTION_FAILED' } }] });
    expect(Date.parse(status.nextRetryAt!) - Date.now()).toBe(60_000);
    expect((await client.getOverview(query)).usage).toEqual(before.usage);
    await vi.advanceTimersByTimeAsync(60_000); expect(queued(events)).toHaveLength(2);
    expect((await client.getTimezone()).rebuild).toBe('rebuilding');
    await vi.advanceTimersByTimeAsync(1000); expect((await client.getTimezone()).rebuild).toBe('backoff');
    remove(); expect(vi.getTimerCount()).toBe(0);
    const again = await client.subscribeUsage(() => {}); expect(vi.getTimerCount()).toBe(1); again(); expect(vi.getTimerCount()).toBe(0);
  });
  it('a cancelled rebuild job backs off and later completes', async () => {
    const client = new MockUsageClient('partial', 0, 1000); await enableOnly(client, [first]);
    vi.useFakeTimers(); const remove = await client.subscribeUsage(() => {});
    client.simulateSystemTimezone('Asia/Tokyo');
    await client.cancelScan((await client.getTimezone()).providers[0].jobId!);
    expect(await client.getTimezone()).toMatchObject({ rebuild: 'backoff', providers: [{ providerId: first, state: 'failed', error: { code: 'CANCELLED' } }] });
    await vi.advanceTimersByTimeAsync(61_000);
    expect(await client.getTimezone()).toMatchObject({ effectiveTimezone: 'Asia/Tokyo', rebuild: 'idle', providers: [] });
    remove(); expect(vi.getTimerCount()).toBe(0);
  });
  it('changes zone without any scan when no sources are enabled; legacy writes of the effective zone keep the mode', async () => {
    const client = new MockUsageClient('partial', 0); await enableOnly(client, []);
    const events: UsageEvent[] = []; const remove = await client.subscribeUsage(event => events.push(event));
    client.simulateSystemTimezone('Asia/Tokyo');
    expect(await client.getTimezone()).toMatchObject({ effectiveTimezone: 'Asia/Tokyo', rebuild: 'idle', providers: [], revision: '3' });
    expect(queued(events)).toHaveLength(0);
    const settings = await client.getSettings(); expect(settings.timezone).toBe('Asia/Tokyo');
    expect(await client.updateSettings({ expectedRevision: settings.revision, timezone: 'Asia/Tokyo', providers: [] })).toMatchObject({ timezone: 'Asia/Tokyo', revision: '4' });
    expect(await client.getTimezone()).toMatchObject({ mode: 'follow-system', effectiveTimezone: 'Asia/Tokyo', revision: '4' });
    remove();
  });
  it('legacy updateSettings with a different zone means fixed at that zone and rebuilds', async () => {
    const client = new MockUsageClient('partial', 0, 1000); await enableOnly(client, [first]);
    vi.useFakeTimers(); const events: UsageEvent[] = []; const remove = await client.subscribeUsage(event => events.push(event));
    client.simulateSystemTimezone('Asia/Tokyo'); await vi.advanceTimersByTimeAsync(1000);
    let settings = await client.getSettings(); expect(settings.timezone).toBe('Asia/Tokyo');
    await expect(client.updateSettings({ expectedRevision: settings.revision, timezone: 'Europe/Berlin', providers: [] })).rejects.toMatchObject({ code: 'UNSUPPORTED_FILTER' });
    settings = await client.updateSettings({ expectedRevision: settings.revision, timezone: DEMO_TIMEZONE, providers: [] });
    expect(settings.timezone).toBe(DEMO_TIMEZONE);
    expect(await client.getTimezone()).toMatchObject({ mode: 'fixed', effectiveTimezone: DEMO_TIMEZONE, revision: settings.revision, rebuild: 'rebuilding' });
    await vi.advanceTimersByTimeAsync(1000); expect((await client.getTimezone()).rebuild).toBe('idle');
    expect(queued(events).map(scan => scan.providerId)).toEqual([first, first]);
    remove(); expect(vi.getTimerCount()).toBe(0);
  });
});

describe('API 1.3 timezone Tauri transport boundary', () => {
  it('older services reject both methods having sent only getApiInfo', async () => {
    const sent: Array<{ command: string; args?: Record<string, unknown> }> = [];
    const send: CommandInvoker = async <T,>(command: string, args?: Record<string, unknown>) => { sent.push({ command, args }); return legacyInfo as T; };
    const client = new TauriUsageClient(send);
    await expect(client.getTimezone()).rejects.toMatchObject({ code: 'UNSUPPORTED_FILTER' });
    await expect(client.updateTimezone({ expectedRevision: '1', mode: 'follow-system', timezone: null })).rejects.toMatchObject({ code: 'UNSUPPORTED_FILTER' });
    expect(sent).toEqual([{ command: COMMANDS.getApiInfo, args: undefined }]);
  });
  it('uses generated commands, exact { request } envelopes and version-checks responses', async () => {
    const calls: Array<{ command: string; args?: Record<string, unknown> }> = [];
    let version = API_VERSION as string;
    const send: CommandInvoker = async <T,>(command: string, args?: Record<string, unknown>) => {
      calls.push({ command, args });
      return (command === COMMANDS.getApiInfo ? DEMO_API_INFO : { ...wireStatus, apiVersion: version }) as T;
    };
    const client = new TauriUsageClient(send);
    const request: UpdateTimezoneRequest = { expectedRevision: '4', mode: 'fixed', timezone: 'Europe/Berlin' };
    expect(await client.getTimezone()).toEqual(wireStatus);
    expect(await client.updateTimezone(request)).toEqual(wireStatus);
    expect(calls).toEqual([{ command: COMMANDS.getApiInfo, args: undefined }, { command: COMMANDS.getTimezone, args: undefined }, { command: COMMANDS.updateTimezone, args: { request } }]);
    version = '2.0.0'; await expect(client.getTimezone()).rejects.toMatchObject({ code: 'API_VERSION_UNSUPPORTED' });
  });
  it('subscribes to TIMEZONE_EVENT only with the capability and removes every listener and timer', async () => {
    vi.useFakeTimers();
    const transport = (info: ApiInfo) => {
      const listeners = new Map<string, (payload: unknown) => void>(); const removed = vi.fn();
      const listen: EventSubscriber = async (name, callback) => { listeners.set(name, callback); return () => { listeners.delete(name); removed(); }; };
      const send: CommandInvoker = async <T,>() => info as T;
      return { listeners, removed, client: new TauriUsageClient(send, 1, listen, 1000) };
    };
    const legacy = transport(legacyInfo);
    const stopLegacy = await legacy.client.subscribeUsage(() => {});
    expect([...legacy.listeners.keys()]).toEqual([SCAN_EVENT, AUTO_COLLECTION_EVENT]);
    stopLegacy(); expect(legacy.listeners.size).toBe(0);
    const current = transport(DEMO_API_INFO); const events: UsageEvent[] = [];
    const stop = await current.client.subscribeUsage(event => events.push(event));
    expect([...current.listeners.keys()]).toEqual([SCAN_EVENT, AUTO_COLLECTION_EVENT, TIMEZONE_EVENT]);
    current.listeners.get(TIMEZONE_EVENT)!(wireStatus);
    expect(events).toContainEqual({ kind: 'timezone', status: wireStatus });
    expect(() => current.listeners.get(TIMEZONE_EVENT)!({ ...wireStatus, apiVersion: '2.0.0' })).toThrow();
    expect(statuses(events)).toHaveLength(1);
    stop(); expect(current.listeners.size).toBe(0); expect(current.removed).toHaveBeenCalledTimes(3); expect(vi.getTimerCount()).toBe(0);
  });
});
