import { renderToStaticMarkup } from 'react-dom/server';
import { describe, expect, it } from 'vitest';
import { MetricValue, QualityBadge, ErrorNotice, LoadingState, StatusBadge } from './ui';
import { formatTokens, formatUsd, barPercent } from './format';
import { Overview } from '../features/Overview';
import { Providers } from '../features/Providers';
import { Sessions } from '../features/Sessions';
import { Settings } from '../features/Settings';
import { MockUsageClient } from '../api/transports/mock/MockUsageClient';
import { DEMO_API_INFO, DEMO_RANGE, DEMO_TIMEZONE, PROVIDERS, reported, unavailable } from '../api/transports/mock/fixtures';
import { apiError } from '../api/protocol';
import { rangeForPreset } from '../features/dates';

describe('presentation accuracy and capabilities', () => {
  it('renders unknown values differently from real zero, with field quality', () => {
    expect(renderToStaticMarkup(<MetricValue metric={unavailable()} />)).toContain('不可用');
    expect(renderToStaticMarkup(<MetricValue metric={reported('0')} />)).toContain('>0<');
    expect(renderToStaticMarkup(<QualityBadge metric={reported('12', 'estimated', 2)} />)).toContain('已知部分');
  });
  it('formats large decimal token strings exactly and only rounds money for display', () => {
    expect(formatTokens('18446744073709551615')).toBe('18,446,744,073,709,551,615');
    expect(formatUsd('12.46500000')).toBe('$12.47');
    expect(formatUsd('0')).toBe('$0.00');
    expect(formatUsd(null)).toBe('不可用');
    expect(formatUsd('0.00000400')).toBe('< $0.01');
    expect(barPercent('18446744073709551615', '18446744073709551615')).toBe(100);
  });
  it('renders loading, permission errors and every required provider/scan state', () => {
    expect(renderToStaticMarkup(<LoadingState />)).toContain('aria-busy="true"');
    expect(renderToStaticMarkup(<ErrorNotice error={apiError('PERMISSION_DENIED', '权限不足')} />)).toContain('role="alert"');
    for (const state of ['not-detected', 'no-data', 'ready', 'scanning', 'permission-denied', 'schema-unsupported', 'partial', 'stale', 'error', 'queued', 'running', 'succeeded', 'failed', 'cancelled'] as const) {
      expect(renderToStaticMarkup(<StatusBadge state={state} />)).toContain(`state-${state}`);
    }
  });
  it('renders partial and stale overview results without summing reasoning or filling missing costs', async () => {
    const result = await new MockUsageClient('stale', 0).getOverview({ range: DEMO_RANGE, timezone: DEMO_TIMEZONE, providerIds: [], modelIds: [], bucket: 'day' });
    const html = renderToStaticMarkup(<Overview result={result} providers={PROVIDERS} timezone={DEMO_TIMEZONE} />);
    expect(html).toContain('1,480,000');
    expect(html).toContain('API 等价估算成本');
    expect(html).toContain('上次成功的快照');
    expect(html).toContain('覆盖不完整');
    expect(html).toContain('推理输出（输出子集）');
    expect(html).toContain('demo-unpriced-model');
    expect(html).toContain('价格日期：未知');
  });
  it('renders successful empty and unavailable fields separately', async () => {
    const result = await new MockUsageClient('empty', 0).getOverview({ range: DEMO_RANGE, timezone: DEMO_TIMEZONE, providerIds: [], modelIds: [], bucket: 'day' });
    const html = renderToStaticMarkup(<Overview result={result} providers={PROVIDERS} timezone={DEMO_TIMEZONE} />);
    expect(html).toContain('所选范围暂无记录');
    expect(html).toContain('>0<');
    expect(html).toContain('不可用');
  });
  it('disables scans for disabled sources and exposes coverage/capabilities', () => {
    const html = renderToStaticMarkup(<Providers providers={[{ ...PROVIDERS[0], enabled: false }]} timezone={DEMO_TIMEZONE} canScan busy={false} onScan={() => {}} />);
    expect(html).toMatch(/<button[^>]*disabled=""/);
    expect(html).toContain('额度不可用');
    expect(html).toContain('完整快照');
  });
  it('states lifetime session semantics, disables period usage and never invents start dates', async () => {
    const result = await new MockUsageClient('partial', 0).listSessions({ timezone: DEMO_TIMEZONE, providerIds: [], modelIds: [], activeRange: null, offset: 0, limit: 20 });
    const html = renderToStaticMarkup(<Sessions result={result} providers={PROVIDERS} timezone={DEMO_TIMEZONE} activeOnly={false} onActiveOnly={() => {}} offset={0} onNext={() => {}} onPrevious={() => {}} supported />);
    expect(html).toContain('全会话累计量');
    expect(html).toContain('来源未提供');
    expect(html).toContain('demo-local-device');
    expect(html).toMatch(/<input[^>]*disabled=""/);
  });
  it('disables unsupported sessions, settings writes and model-filtered JSON exports', () => {
    expect(renderToStaticMarkup(<Sessions providers={PROVIDERS} timezone={DEMO_TIMEZONE} activeOnly={false} onActiveOnly={() => {}} offset={0} onNext={() => {}} onPrevious={() => {}} supported={false} />)).toContain('此来源不支持会话报表');
    const html = renderToStaticMarkup(<Settings theme="dark" setTheme={() => {}} timezone={DEMO_TIMEZONE} setTimezone={() => {}} demo providers={PROVIDERS} api={DEMO_API_INFO} modelFiltered exporting={false} onExport={() => {}} />);
    expect(html).toContain('未开放修改');
    expect(html).toContain('不受日期范围限制');
    expect(html).toMatch(/<button[^>]*disabled=""[^>]*>.*?导出 JSON/s);
    expect(html).toContain('不会保存文件');
  });
  it('uses half-open dates and ISO weeks across the year boundary', () => {
    expect(rangeForPreset('2027-01-01', 'week')).toEqual({ start: '2026-12-28', end: '2027-01-02' });
    expect(rangeForPreset('2026-12-31', 'month')).toEqual({ start: '2026-12-01', end: '2027-01-01' });
  });
  it('renders settings capabilities, safe directory labels and scan-time write blocking', async () => {
    const settings = await new MockUsageClient('partial', 0).getSettings();
    const html = renderToStaticMarkup(<Settings theme="dark" setTheme={() => {}} timezone={DEMO_TIMEZONE} setTimezone={() => {}} demo providers={PROVIDERS} api={DEMO_API_INFO} modelFiltered={false} exporting={false} onExport={() => {}} controls={{ value: settings, pending: false, choosing: false, busy: true, save: () => {}, choose: async () => null, reload: () => {} }} />);
    expect(html).toContain('扫描进行中，结束后可修改设置');
    expect(html).toContain('关闭来源会保留已有历史');
    expect(html).toContain('默认日志目录');
    expect(html).toMatch(/aria-label="启用 Claude Code"[^>]*disabled=""/);
  });
});
