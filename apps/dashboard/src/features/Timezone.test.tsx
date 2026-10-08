import { afterEach, describe, expect, it, vi } from 'vitest';
import type { ReactNode } from 'react';
import { renderToStaticMarkup } from 'react-dom/server';
import { QueryClient, QueryClientProvider } from '@tanstack/react-query';
import { TimezoneSettings, timezoneOptions, timezoneRequest } from './TimezoneSettings';
import { TimezoneNotice } from './TimezoneNotice';
import { acceptTimezoneStatus, applyBackgroundEvent, resolveFetchedTimezone } from './useUsage';
import { I18nProvider } from '../i18n/I18nContext';
import { apiError } from '../api/protocol';
import { API_VERSION, type AutoCollectionStatus, type SettingsResult, type TimezoneStatus } from '../api/generated/usage';
import { App } from '../app/App';
import { UsageProvider } from '../app/UsageContext';
import { MockUsageClient } from '../api/transports/mock/MockUsageClient';
import { DEMO_API_INFO, PROVIDERS } from '../api/transports/mock/fixtures';

afterEach(() => { vi.unstubAllGlobals(); });
const han = /[\p{Script=Han}]/u;
const status: TimezoneStatus = { apiVersion: API_VERSION, sequence: '10', revision: '4', mode: 'follow-system', effectiveTimezone: 'America/Phoenix', systemTimezone: 'America/Phoenix', detectionError: null, pendingTimezone: null, rebuild: 'idle', nextRetryAt: null, providers: [] };
const render = (language: 'zh' | 'en', node: ReactNode) => renderToStaticMarkup(<I18nProvider initialLanguage={language}>{node}</I18nProvider>);
const dependents = ['settings', 'providers', 'overview', 'sessions', 'auto'];
const checkedMode = (html: string) => /<input(?=[^>]*checked="")[^>]*value="([^"]+)"/.exec(html)?.[1];
const settingsAt = (revision: string, timezone: string): SettingsResult => ({ apiVersion: API_VERSION, revision, timezone, providers: [], collectionNotice: '', directoryChangePolicy: 'preserve-dataset' });
const autoAt = (revision: string, timezone: string): AutoCollectionStatus => ({ apiVersion: API_VERSION, revision, timezone, config: { enabled: false, intervalMinutes: 5 }, providers: [] });
const invalidatedIn = (cache: QueryClient) => ['api', ...dependents].filter(name => cache.getQueryState(['usage', name])?.isInvalidated);
/** What useTimezone's query does with a getTimezone response: resolve it, then store the result. */
const fetchedInto = (cache: QueryClient, fetched: TimezoneStatus) => { const kept = resolveFetchedTimezone(cache, fetched); cache.setQueryData(['usage', 'timezone'], kept); return kept === fetched; };

describe('timezone status cache ordering and invalidation', () => {
  it('ignores older sequences and invalidates only when zone, revision, mode or a finished rebuild changes data', () => {
    const cache = new QueryClient();
    // Revision-guarded editors hold the same shared revision and zone as the first status, as when they load together.
    const seeded = (name: string) => name === 'settings' ? settingsAt('4', 'America/Phoenix') : name === 'auto' ? autoAt('4', 'America/Phoenix') : { marker: name };
    const seed = () => { for (const name of ['api', ...dependents]) cache.setQueryData(['usage', name], seeded(name)); };
    const invalidated = () => ['api', ...dependents].filter(name => cache.getQueryState(['usage', name])?.isInvalidated);
    const next = (change: Partial<TimezoneStatus>) => { const current = cache.getQueryData<TimezoneStatus>(['usage', 'timezone'])!; return { ...current, ...change, sequence: (BigInt(current.sequence) + 1n).toString() }; };
    seed();
    expect(acceptTimezoneStatus(cache, status)).toBe(true); expect(invalidated()).toEqual([]);
    // Decimal sequences compare numerically ('9' < '10'); equal or older statuses never regress the cache.
    expect(acceptTimezoneStatus(cache, { ...status, sequence: '9', effectiveTimezone: 'Asia/Tokyo' })).toBe(false);
    expect(acceptTimezoneStatus(cache, { ...status, effectiveTimezone: 'Asia/Tokyo' })).toBe(false);
    expect(cache.getQueryData(['usage', 'timezone'])).toEqual(status); expect(invalidated()).toEqual([]);
    expect(acceptTimezoneStatus(cache, next({ rebuild: 'rebuilding', providers: [{ providerId: 'fixture', state: 'rebuilding', jobId: 'job', error: null }] }))).toBe(true);
    expect(invalidated()).toEqual([]);
    acceptTimezoneStatus(cache, next({ rebuild: 'idle', providers: [] })); expect(invalidated()).toEqual(dependents);
    for (const change of [{ effectiveTimezone: 'Asia/Tokyo' }, { revision: '5' }, { mode: 'fixed' as const }]) {
      seed(); acceptTimezoneStatus(cache, next(change)); expect(invalidated()).toEqual(dependents);
    }
    seed(); expect(acceptTimezoneStatus(cache, { ...status, sequence: '18446744073709551616' })).toBe(true);
    expect(invalidated()).toEqual(dependents); expect(acceptTimezoneStatus(cache, { ...status, sequence: '18446744073709551615' })).toBe(false); cache.clear();
  });
  it('routes background timezone events through the sequence guard', () => {
    const cache = new QueryClient(); for (const name of ['api', 'overview']) cache.setQueryData(['usage', name], { marker: name });
    applyBackgroundEvent(cache, { kind: 'timezone', status }); expect(cache.getQueryData(['usage', 'timezone'])).toEqual(status);
    applyBackgroundEvent(cache, { kind: 'timezone', status: { ...status, sequence: '3', effectiveTimezone: 'UTC' } });
    expect(cache.getQueryData<TimezoneStatus>(['usage', 'timezone'])?.effectiveTimezone).toBe('America/Phoenix');
    expect(cache.getQueryState(['usage', 'overview'])?.isInvalidated).toBe(false);
    applyBackgroundEvent(cache, { kind: 'timezone', status: { ...status, sequence: '11', effectiveTimezone: 'UTC', revision: '5' } });
    expect(cache.getQueryData<TimezoneStatus>(['usage', 'timezone'])?.effectiveTimezone).toBe('UTC');
    expect(cache.getQueryState(['usage', 'overview'])?.isInvalidated).toBe(true);
    expect(cache.getQueryState(['usage', 'api'])?.isInvalidated).toBe(false); cache.clear();
  });
  it('reconciles Settings and Automatic collection cached before the first status, once, for events and getTimezone alike', () => {
    const tokyo: TimezoneStatus = { ...status, sequence: '1', revision: '5', effectiveTimezone: 'Asia/Tokyo', systemTimezone: 'Asia/Tokyo' };
    const cases: { settings?: SettingsResult; auto?: AutoCollectionStatus; stale: boolean }[] = [
      { settings: settingsAt('4', 'America/Phoenix'), stale: true }, // an older shared revision and zone
      { settings: settingsAt('5', 'America/Phoenix'), auto: autoAt('5', 'Asia/Tokyo'), stale: true }, // only the zone differs
      { auto: autoAt('4', 'Asia/Tokyo'), stale: true }, // only the revision differs
      { settings: settingsAt('5', 'Asia/Tokyo'), auto: autoAt('5', 'Asia/Tokyo'), stale: false }, // an already matching snapshot
      { stale: false }, // nothing revision-guarded was cached yet
    ];
    for (const deliver of [acceptTimezoneStatus, fetchedInto]) for (const { settings, auto, stale } of cases) {
      const cache = new QueryClient();
      // setQueryData also clears an earlier invalidation, so loading again re-arms the checks.
      const load = () => {
        for (const name of ['api', 'providers', 'overview', 'sessions']) cache.setQueryData(['usage', name], { marker: name });
        if (settings) cache.setQueryData(['usage', 'settings'], settings); if (auto) cache.setQueryData(['usage', 'auto'], auto);
      };
      load(); const cached = dependents.filter(name => cache.getQueryData(['usage', name]) !== undefined);
      expect(deliver(cache, tokyo)).toBe(true); expect(cache.getQueryData(['usage', 'timezone'])).toEqual(tokyo);
      expect(invalidatedIn(cache)).toEqual(stale ? cached : []);
      // Only the first status reconciles: while the old data is still cached (its refresh has not landed or
      // failed), unchanged polling and older or equal statuses never invalidate again.
      load();
      for (const sequence of ['2', '3', '4']) expect(deliver(cache, { ...tokyo, sequence })).toBe(true);
      expect(deliver(cache, { ...tokyo, sequence: '4' })).toBe(false); expect(deliver(cache, { ...status, sequence: '3' })).toBe(false);
      expect(cache.getQueryData<TimezoneStatus>(['usage', 'timezone'])).toEqual({ ...tokyo, sequence: '4' }); expect(invalidatedIn(cache)).toEqual([]);
      cache.clear();
    }
  });
  it('refreshes Settings read before the first host status, so source saves use the current shared revision', async () => {
    const client = new MockUsageClient('partial', 0);
    const initial = await client.getSettings(); // no enabled sources: the switch below starts no rebuild scan
    await client.updateSettings({ expectedRevision: initial.revision, timezone: initial.timezone, providers: initial.providers.map(({ providerId }) => ({ providerId, enabled: false, directoryRef: null })) });
    const cache = new QueryClient();
    const settings = { queryKey: ['usage', 'settings'], queryFn: () => client.getSettings(), staleTime: Infinity };
    const timezone = { queryKey: ['usage', 'timezone'], queryFn: async () => resolveFetchedTimezone(cache, await client.getTimezone()) };
    const before = await cache.fetchQuery(settings); expect(before).toMatchObject({ revision: '2', timezone: 'America/Phoenix' });
    client.simulateSystemTimezone('Asia/Tokyo');
    expect(await cache.fetchQuery(timezone)).toMatchObject({ revision: '3', effectiveTimezone: 'Asia/Tokyo' });
    // Without reconciliation the editor would keep revision 2, which the host rejects.
    await expect(client.updateSettings({ expectedRevision: before.revision, timezone: before.timezone, providers: [] })).rejects.toMatchObject({ code: 'SETTINGS_CONFLICT' });
    expect(cache.getQueryState(['usage', 'settings'])?.isInvalidated).toBe(true);
    const fresh = await cache.fetchQuery(settings); expect(fresh).toMatchObject({ revision: '3', timezone: 'Asia/Tokyo' });
    for (let poll = 0; poll < 3; poll++) await cache.fetchQuery(timezone);
    expect(cache.getQueryState(['usage', 'settings'])?.isInvalidated).toBe(false);
    expect(await client.updateSettings({ expectedRevision: fresh.revision, timezone: fresh.timezone, providers: [] })).toMatchObject({ revision: '4', timezone: 'Asia/Tokyo' });
    cache.clear();
  });
});

describe('timezone settings and notices in both languages', () => {
  const controls = { status, pending: false, save: () => {}, reload: () => {} };
  it('renders the mode radio group, effective/system zones and the fixed-zone select', () => {
    const zh = render('zh', <TimezoneSettings controls={controls} demo={false} />);
    expect(zh).toContain('<legend>统计时区模式</legend>'); expect(zh).toContain('跟随系统时区（推荐）');
    expect(zh).toContain('<p role="status">当前统计时区：America/Phoenix'); expect(zh).toContain('系统时区：America/Phoenix');
    expect(zh).toMatch(/<select aria-label="固定统计时区" disabled="">/); expect(checkedMode(zh)).toBe('follow-system');
    expect(zh).toContain('已汇总的每日总量不会被直接改标');
    const en = render('en', <TimezoneSettings controls={controls} demo />);
    expect(en).not.toMatch(han);
    expect(en).toContain('<legend>Reporting timezone mode</legend>'); expect(en).toContain('Follow the system timezone (recommended)');
    expect(en).toContain('Reporting timezone in use: America/Phoenix'); expect(en).toContain('System timezone: America/Phoenix');
    expect(en).toContain('aria-label="Fixed reporting timezone"'); expect(en).toContain('Aggregated daily totals are never relabeled');
    expect(en).toContain('Demo data is available only for America/Phoenix.');
    const fixed = render('en', <TimezoneSettings controls={{ ...controls, status: { ...status, mode: 'fixed', effectiveTimezone: 'Europe/Berlin', systemTimezone: null, detectionError: apiError('INTERNAL', '检测失败', true) } }} demo={false} />);
    expect(fixed).not.toMatch(han);
    expect(fixed).toContain('<select aria-label="Fixed reporting timezone">'); expect(fixed).toContain('<option value="Europe/Berlin" selected="">Europe/Berlin</option>');
    expect(checkedMode(fixed)).toBe('fixed'); expect(fixed).toContain('System timezone: unable to detect'); expect(fixed).toContain('latest detection failed');
    expect(timezoneOptions('Mars/Base', null)).toEqual(expect.arrayContaining(['UTC', 'America/Phoenix', 'Mars/Base']));
    expect(timezoneRequest(status, 'fixed', 'Europe/Berlin')).toEqual({ expectedRevision: '4', mode: 'fixed', timezone: 'Europe/Berlin' });
    expect(timezoneRequest(status, 'follow-system', 'Europe/Berlin')).toEqual({ expectedRevision: '4', mode: 'follow-system', timezone: null });
  });
  it('shows loading and errors without editable controls', () => {
    const loading = render('en', <TimezoneSettings controls={{ ...controls, status: undefined }} demo={false} />);
    expect(loading).toContain('Loading reporting timezone…'); expect(loading).not.toContain('timezone-mode');
    const failed = render('en', <TimezoneSettings controls={{ ...controls, status: undefined, error: apiError('SETTINGS_CONFLICT', '设置已变化') }} demo={false} />);
    expect(failed).not.toMatch(han); expect(failed).toContain('Settings changed. Reload them before saving.'); expect(failed).not.toContain('Loading reporting timezone');
  });
  it('renders every rebuild state with per-source progress, cancel and retry time', () => {
    expect(render('en', <TimezoneNotice status={status} names={{}} />)).toBe('');
    for (const rebuild of ['pending', 'rebuilding', 'backoff'] as const) {
      const view: TimezoneStatus = { ...status, effectiveTimezone: 'Asia/Tokyo', rebuild, nextRetryAt: rebuild === 'backoff' ? '2026-10-05T18:01:00Z' : null,
        providers: [{ providerId: 'fixture', state: rebuild === 'backoff' ? 'failed' : rebuild, jobId: rebuild === 'rebuilding' ? 'job-1' : null, error: rebuild === 'backoff' ? apiError('COLLECTION_FAILED', '演示扫描失败', true) : null }, { providerId: 'other', state: 'succeeded', jobId: null, error: null }] };
      const notice = (language: 'zh' | 'en') => render(language, <TimezoneNotice status={view} names={{ fixture: 'Fixture' }} onCancel={() => {}} />);
      const en = notice('en'); const zh = notice('zh');
      expect(en).not.toMatch(han); expect(en).toContain('rebuilt from their source logs for Asia/Tokyo'); expect(en).toContain('missing values stay unavailable');
      expect(en).toContain('aria-label="Rebuild progress by source"'); expect(en).toContain('other · Rebuilt');
      expect(zh).toContain('旧时区的结果不会作为新时区数据显示'); expect(zh).toContain('aria-label="各来源重建进度"');
      expect(en.includes('Cancel rebuild')).toBe(rebuild === 'rebuilding'); expect(zh.includes('取消重建')).toBe(rebuild === 'rebuilding');
      if (rebuild === 'pending') { expect(en).toContain('Waiting to rebuild for the new reporting timezone'); expect(en).toContain('Fixture · Waiting'); }
      if (rebuild === 'rebuilding') { expect(en).toContain('Rebuilding for the new reporting timezone'); expect(en).toContain('Fixture · Rebuilding'); }
      if (rebuild === 'backoff') {
        expect(en).toContain('notice-warning'); expect(en).toContain('Retrying automatically at 10/06, 03:01'); expect(zh).toContain('10/06 03:01');
        expect(en).toContain('Fixture · Rebuild failed'); expect(en).toContain('Previous successful data was retained'); expect(zh).toContain('演示扫描失败');
      }
    }
    const disabled = render('en', <TimezoneNotice status={{ ...status, rebuild: 'rebuilding', providers: [{ providerId: 'fixture', state: 'rebuilding', jobId: 'job-1', error: null }] }} names={{}} cancelling onCancel={() => {}} />);
    expect(disabled).toMatch(/<button[^>]*disabled=""[^>]*>Cancelling…/);
  });
  it('explains pending switches and detection failures', () => {
    const view = { ...status, pendingTimezone: 'Asia/Tokyo', detectionError: apiError('INTERNAL', '无法检测', true) };
    const en = render('en', <TimezoneNotice status={view} names={{}} />); const zh = render('zh', <TimezoneNotice status={view} names={{}} />);
    expect(en).not.toMatch(han);
    expect(en).toContain('The system timezone changed to Asia/Tokyo. The reporting timezone will switch after the current scan finishes.');
    expect(en).toContain('Unable to detect the system timezone'); expect(en).toContain('The current reporting timezone America/Phoenix is kept');
    expect(zh).toContain('将在当前扫描结束后切换统计时区'); expect(zh).toContain('无法检测系统时区'); expect(zh).toContain('继续使用当前统计时区 America/Phoenix');
  });
});

describe('app timezone wiring', () => {
  it('derives the query zone from host status during render and keeps the legacy row for older services', async () => {
    const client = new MockUsageClient('partial', 0);
    const cache = new QueryClient({ defaultOptions: { queries: { retry: false, staleTime: Infinity } } });
    cache.setQueryData(['usage', 'api'], DEMO_API_INFO); cache.setQueryData(['usage', 'providers'], PROVIDERS); cache.setQueryData(['usage', 'settings'], await client.getSettings());
    cache.setQueryData(['usage', 'timezone'], { ...await client.getTimezone(), effectiveTimezone: 'Asia/Tokyo', systemTimezone: 'Asia/Tokyo', rebuild: 'rebuilding', providers: [{ providerId: PROVIDERS[0].providerId, state: 'rebuilding', jobId: 'demo-scan-9', error: null }] });
    const page = () => renderToStaticMarkup(<I18nProvider initialLanguage="en"><QueryClientProvider client={cache}><UsageProvider client={client}><App demo /></UsageProvider></QueryClientProvider></I18nProvider>);
    let html = page();
    expect(html).toContain('Asia/Tokyo'); expect(html).not.toContain('America/Phoenix');
    expect(html).toContain('Claude Code · Rebuilding'); expect(html).toContain('Cancel rebuild'); expect(html).not.toMatch(han);
    vi.stubGlobal('window', { location: { hash: '#settings' } });
    html = page(); expect(html).toContain('Reporting timezone in use: Asia/Tokyo'); expect(html).not.toContain('aria-label="Reporting timezone"');
    cache.setQueryData(['usage', 'api'], { ...DEMO_API_INFO, capabilities: DEMO_API_INFO.capabilities.filter(capability => capability !== 'timezone-follow-system') });
    html = page(); expect(html).toContain('aria-label="Reporting timezone"'); expect(html).not.toContain('timezone-mode'); expect(html).not.toContain('Asia/Tokyo');
    cache.clear();
  });
});
