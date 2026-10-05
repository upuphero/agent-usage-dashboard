import { describe, expect, it } from 'vitest';
import { renderToStaticMarkup } from 'react-dom/server';
import { chartGeometry, TrendChart } from './TrendChart';
import { formatChartCount, formatChineseMagnitude } from '../components/format';
import { MetricCard } from '../components/ui';
import { MockUsageClient } from '../api/transports/mock/MockUsageClient';
import { DEMO_RANGE, DEMO_TIMEZONE, reported, ALL_USAGE } from '../api/transports/mock/fixtures';
import { FilterPopover } from '../components/FilterPopover';
import { Overview } from './Overview';

describe('chart readability and exact token presentation', () => {
  it('keeps integer precision and gates Chinese reading aids by UI locale', () => {
    expect(formatChineseMagnitude('1855654157')).toBe('约18亿5565万');
    expect(formatChineseMagnitude('18446744073709551615')).toBe('约1844京6744兆');
    expect(formatChineseMagnitude('180000000')).toBe('1亿8000万');
    expect(formatChineseMagnitude('10000')).toBe('1万');
    expect(formatChineseMagnitude('0')).toBeNull();
    expect(formatChineseMagnitude(null)).toBeNull();
    expect(formatChineseMagnitude('1855654157', 'en-US')).toBeNull();
    const html = renderToStaticMarkup(<MetricCard label="总量" metric={reported('1855654157')} note="" />);
    expect(html).toContain('1,855,654,157'); expect(html).toContain('约18亿5565万');
    expect(formatChartCount('1855654157')).toBe('18.6亿');
  });
  it('breaks lines for unavailable or absent periods and keeps zero at the baseline', () => {
    const point = (start: string, value: string | null) => ({ start, usage: { ...ALL_USAGE, tokens: { ...ALL_USAGE.tokens, total: { ...reported('0'), value, accuracy: value === null ? 'unavailable' as const : 'exact' as const } } } });
    const geometry = chartGeometry([point('2026-01-01', '0'), point('2026-01-02', '18446744073709551615'), point('2026-01-03', null), point('2026-01-04', '10'), point('2026-01-06', '20')], 'day');
    expect(geometry.maximum).toBe('18446744073709551615');
    expect(geometry.points[0].y).toBe(240); expect(geometry.points[1].y).toBe(0); expect(geometry.points[2].y).toBeNull();
    expect(geometry.segments.map(segment => segment.length)).toEqual([2, 1, 1]);
    const knownZero = chartGeometry([point('2026-01-01','10'),point('2026-01-02','0'),point('2026-01-03','20')],'day');
    expect(knownZero.segments).toHaveLength(1);expect(knownZero.points[1].y).toBe(240);
  });
  it('puts the chart first and leaves full point values in accessible hover targets, not a crowded row', async () => {
    const result = await new MockUsageClient('partial', 0).getOverview({ range: DEMO_RANGE, timezone: DEMO_TIMEZONE, providerIds: [], modelIds: [], bucket: 'day' });
    const chart = renderToStaticMarkup(<TrendChart result={result} />);
    expect(chart).toContain('aria-label="曲线图" aria-pressed="true"');
    expect(chart).toContain('aria-label="柱状图"');
    expect(chart).not.toContain('class="trend-value"');
    expect(chart).not.toContain('role="tooltip"');
    expect(chart).toContain('2026-09-28：118,000 token');
    const html = renderToStaticMarkup(<Overview result={result} providers={[]} timezone={DEMO_TIMEZONE} />);
    expect(html.indexOf('用量趋势')).toBeLessThan(html.indexOf('已知总 token'));
    const filters = renderToStaticMarkup(<FilterPopover summary="全部来源"><select aria-label="日期范围"><option>30天</option></select></FilterPopover>);
    expect(filters).toContain('aria-expanded="false"'); expect(filters).not.toContain('<select');
  });
});
