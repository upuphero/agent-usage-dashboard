import { renderToStaticMarkup } from 'react-dom/server';
import { QueryClient, QueryClientProvider } from '@tanstack/react-query';
import { afterEach, describe, expect, it, vi } from 'vitest';
import { I18nProvider, readLanguage } from './I18nContext';
import { english, LANGUAGE_STORAGE_KEY, localizeError, localizeSourceText, translate } from './messages';
import { formatChartCount, formatTokens, qualityDescription } from '../components/format';
import { apiError } from '../api/protocol';
import { App } from '../app/App';
import { UsageProvider } from '../app/UsageContext';
import { MockUsageClient } from '../api/transports/mock/MockUsageClient';
import { DEMO_API_INFO, DEMO_TIMEZONE, PROVIDERS, reported } from '../api/transports/mock/fixtures';
import { rangeForPreset } from '../features/dates';

afterEach(() => { vi.unstubAllGlobals(); });

describe('complete Chinese and English interfaces', () => {
  it('defaults to Chinese and restores only a valid saved language', () => {
    for (const value of [null, '', 'invalid', 'en-US', 'zh']) {
      expect(readLanguage({ getItem: key => { expect(key).toBe(LANGUAGE_STORAGE_KEY); return value; } })).toBe('zh');
    }
    expect(readLanguage({ getItem: () => 'en' })).toBe('en');
    expect(readLanguage({ getItem: () => { throw new Error('storage unavailable'); } })).toBe('zh');
  });

  it('has English copy for every key and interpolates data without changing it', () => {
    for (const value of Object.values(english)) expect(value).not.toMatch(/[\p{Script=Han}]/u);
    expect(translate('en', '已保存 {filename}。', { filename: 'usage.csv' })).toBe('Saved usage.csv.');
    expect(translate('zh', '已保存 {filename}。', { filename: 'usage.csv' })).toBe('已保存 usage.csv。');
    expect(localizeSourceText('C:/日志/模型', 'en')).toBe('C:/日志/模型');
    expect(localizeSourceText('C:/日志/模型 (演示)', 'en')).toBe('C:/日志/模型 (演示)');
    expect(localizeSourceText('已配置自定义用量目录', 'en')).toBe('Configured custom usage directory');
  });

  it('localizes formatting and errors while keeping exact token values', () => {
    expect(formatTokens('18446744073709551615', 'en')).toBe('18,446,744,073,709,551,615');
    expect(formatTokens(null, 'en')).toBe('Unavailable');
    expect(formatChartCount('1855654157', 'zh')).toBe('18.6亿');
    expect(formatChartCount('1855654157', 'en')).toBe('1.9B');
    expect(formatChartCount('1480000', 'en')).toBe('1.5M');
    expect(qualityDescription(reported('12', 'estimated', 2), 'en')).toBe('Estimated · 1 known record · 2 missing; only known values shown');
    expect(translate('en', '{count} 个会话', { count: 1 })).toBe('1 session');
    expect(translate('en', '{count} 个会话', { count: 2 })).toBe('2 sessions');
    expect(localizeError(apiError('PERMISSION_DENIED', '权限不足'), 'en')).toContain('permissions');
    expect(localizeError(apiError('PERMISSION_DENIED', 'permission denied'), 'zh')).toContain('权限不足');
  });

  for (const page of ['overview', 'providers', 'sessions', 'settings']) {
    it(`renders the complete ${page} page in English without Chinese UI remnants`, async () => {
      vi.stubGlobal('window', { location: { hash: `#${page}` } });
      const client = new MockUsageClient('partial', 0);
      const cache = new QueryClient({ defaultOptions: { queries: { retry: false, staleTime: Infinity } } });
      const query = { range: rangeForPreset('2026-10-04', 'last30'), timezone: DEMO_TIMEZONE, bucket: 'day' as const, providerIds: [], modelIds: [] };
      cache.setQueryData(['usage', 'api'], DEMO_API_INFO);
      cache.setQueryData(['usage', 'providers'], PROVIDERS);
      cache.setQueryData(['usage', 'settings'], await client.getSettings());
      cache.setQueryData(['usage', 'overview', query], await client.getOverview(query));
      const sessionQuery = { timezone: DEMO_TIMEZONE, providerIds: PROVIDERS.filter(provider => provider.capabilities.reportKinds.includes('session')).map(provider => provider.providerId), modelIds: [], activeRange: null, offset: 0, limit: 20 };
      cache.setQueryData(['usage', 'sessions', sessionQuery], await client.listSessions(sessionQuery));
      const render = (language: 'zh' | 'en') => renderToStaticMarkup(<I18nProvider initialLanguage={language}><QueryClientProvider client={cache}><UsageProvider client={client}><App demo /></UsageProvider></QueryClientProvider></I18nProvider>);
      const englishHtml = render('en');
      expect(englishHtml).not.toMatch(/[\p{Script=Han}]/u);
      expect(englishHtml).toContain('aria-label="Overview"');
      expect(englishHtml).toContain('aria-label="Switch to Chinese"');
      const chineseHtml = render('zh');
      expect(chineseHtml).toContain('aria-label="概览"');
      expect(chineseHtml).toContain('aria-label="切换到英文"');
      expect(chineseHtml).not.toContain('LOCAL DASHBOARD');
      cache.clear();
    });
  }
});
