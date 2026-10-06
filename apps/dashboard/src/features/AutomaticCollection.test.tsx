import { describe, expect, it } from 'vitest';
import { renderToStaticMarkup } from 'react-dom/server';
import { QueryClient } from '@tanstack/react-query';
import { AutomaticCollection } from './AutomaticCollection';
import { applyBackgroundEvent } from './useUsage';
import { I18nProvider } from '../i18n/I18nContext';
import { API_VERSION, type AutoCollectionStatus, type AutoCollectionState } from '../api/generated/usage';
const status: AutoCollectionStatus = { apiVersion: API_VERSION, revision: '3', timezone: 'America/Phoenix', config: { enabled: true, intervalMinutes: 1 },
  providers: [{ providerId: 'fixture', state: 'scanning', jobId: 'background', lastSuccessAt: '2026-10-05T18:00:00Z', nextCheckAt: '2026-10-05T18:01:00Z', watching: true, error: null }] };
describe('automatic settings presentation and background cache recovery', () => {
  it('renders every state and accessibility label in both languages using the reporting zone', () => {
    for (const state of ['idle', 'checking', 'scanning', 'waiting', 'backoff', 'disabled'] as AutoCollectionState[]) {
      const view = { ...status, providers: [{ ...status.providers[0], state, watching: false }] };
      const render = (language: 'zh' | 'en') => renderToStaticMarkup(<I18nProvider initialLanguage={language}><AutomaticCollection names={{ fixture: 'Fixture' }} controls={{ status: view, pending: false, cancelling: false, save: () => {}, cancel: () => {}, reload: () => {} }} /></I18nProvider>);
      expect(render('en')).not.toMatch(/[\p{Script=Han}]/u); expect(render('en')).toContain('1 minute');
      expect(render('en')).toContain('11:00'); expect(render('zh')).toContain('自动采集');
    }
  });
  it('shows unsupported service explicitly without editable new settings', () => {
    const html = renderToStaticMarkup(<I18nProvider initialLanguage="en"><AutomaticCollection names={{}} /></I18nProvider>);
    expect(html).toContain('does not support automatic'); expect(html).not.toContain('auto-interval');
  });
  it('invalidates source/chart/session data for unseen jobs and reconnects without a cache loop', () => {
    const cache = new QueryClient();
    for (const name of ['api', 'settings', 'providers', 'overview', 'sessions']) cache.setQueryData(['usage', name], { marker: 'old' });
    applyBackgroundEvent(cache, { kind: 'auto', status });
    expect(cache.getQueryState(['usage', 'overview'])?.isInvalidated).toBe(false);
    applyBackgroundEvent(cache, { kind: 'scan', scan: { apiVersion: API_VERSION, jobId: 'unseen', providerId: 'fixture', state: 'succeeded', startedAt: '', finishedAt: '', error: null, snapshotsReplaced: 2, rowsWritten: 5 } });
    for (const name of ['providers', 'overview', 'sessions']) expect(cache.getQueryState(['usage', name])?.isInvalidated).toBe(true);
    expect(cache.getQueryState(['usage', 'settings'])?.isInvalidated).toBe(false);
    expect(cache.getQueryData(['usage', 'auto'])).toEqual(status);
    applyBackgroundEvent(cache, { kind: 'resync' }); expect(cache.getQueryState(['usage', 'api'])?.isInvalidated).toBe(false); cache.clear();
  });
});
