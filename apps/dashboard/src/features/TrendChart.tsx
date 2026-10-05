import { useId, useState } from 'react';
import type { Bucket, OverviewResult } from '../api/generated/usage';
import { Icon } from '../components/Icon';
import { barPercent, formatChartCount, formatChineseMagnitude, formatTokens, largestMetric, qualityDescription } from '../components/format';
import { EmptyState, Panel, QualityBadge } from '../components/ui';

type Series = OverviewResult['buckets'];
const WIDTH = 1000;
const HEIGHT = 240;
function nextPeriod(start: string, bucket: Bucket): string {
  const date = new Date(`${start}T00:00:00Z`);
  if (bucket === 'month') date.setUTCMonth(date.getUTCMonth() + 1);
  else date.setUTCDate(date.getUTCDate() + (bucket === 'week' ? 7 : 1));
  return date.toISOString().slice(0, 10);
}
export function chartGeometry(buckets: Series, bucket: Bucket) {
  const maximum = largestMetric(buckets.map(point => point.usage.tokens.total.value));
  const first = Date.parse(buckets[0]?.start ?? '1970-01-01');
  const last = Date.parse(buckets.at(-1)?.start ?? '1970-01-01');
  const points = buckets.map((point, index) => ({
    ...point, index,
    x: buckets.length === 1 ? WIDTH / 2 : 12 + (Date.parse(point.start) - first) / Math.max(1, last - first) * (WIDTH - 24),
    y: point.usage.tokens.total.value === null ? null : HEIGHT - barPercent(point.usage.tokens.total.value, maximum) / 100 * HEIGHT,
  }));
  const segments: typeof points[] = [];
  for (const point of points) {
    if (point.y === null) continue;
    const previous = points[point.index - 1];
    if (!previous || previous.y === null || nextPeriod(previous.start, bucket) !== point.start) segments.push([]);
    segments.at(-1)!.push(point);
  }
  const paths = segments.map(segment => segment.map((point, index) => {
    if (index === 0) return `M ${point.x} ${point.y}`;
    const previous = segment[index - 1];
    const middle = (previous.x + point.x) / 2;
    // Bounded control points stay between the observed values; never overshoot below zero.
    return `C ${middle} ${previous.y}, ${middle} ${point.y}, ${point.x} ${point.y}`;
  }).join(' '));
  return { maximum, points, segments, paths };
}

export function TrendChart({ result }: { result: OverviewResult }) {
  const [mode, setMode] = useState<'line' | 'bars'>('line');
  const [active, setActive] = useState<string | null>(null);
  const fillId = useId().replace(/[^a-zA-Z0-9_-]/g, '');
  const { points, maximum, paths, segments } = chartGeometry(result.buckets, result.query.bucket);
  const selected = active === null ? null : points.find(point => point.start === active) ?? null;
  const axis = [4, 3, 2, 1, 0].map(tick => ({ position: (4 - tick) * 25, value: maximum === null ? null : (BigInt(maximum) * BigInt(tick) / 4n).toString() }));
  const dateLabel = (date: string) => result.query.bucket === 'month' ? date.slice(0, 7) : date.slice(5);
  const ticks = points.filter((_, index) => index === 0 || index === points.length - 1 || index % Math.max(1, Math.ceil((points.length - 1) / 5)) === 0);
  const period = result.query.bucket === 'day' ? '每日' : result.query.bucket === 'week' ? 'ISO 周' : '每月';
  return <Panel title="用量趋势" eyebrow="ACTIVITY" className="trend-panel" action={<div className="trend-actions"><span className="small muted">{period} · token</span><div className="chart-switch" role="group" aria-label="趋势图类型"><button aria-label="曲线图" aria-pressed={mode === 'line'} onClick={() => setMode('line')}><Icon name="chart-line" /><span>曲线</span></button><button aria-label="柱状图" aria-pressed={mode === 'bars'} onClick={() => setMode('bars')}><Icon name="chart-bars" /><span>柱状</span></button></div></div>}>
    {!points.length ? <EmptyState title={result.usage.tokens.total.value === null ? '尚未采集用量' : '所选范围暂无记录'} description={result.usage.tokens.total.value === null ? '请在来源页面启用并扫描来源；未知值保留为不可用。' : '已知范围没有记录；未提供的字段仍显示为不可用。'} /> : <>
      <div className="trend-surface" onPointerLeave={event => { if (event.pointerType !== 'touch') setActive(null); }}>
        <div className="trend-y-axis" aria-hidden="true">{axis.map(tick => <span key={tick.position} style={{ top: `${tick.position}%` }}>{tick.value === null ? '—' : formatChartCount(tick.value)}</span>)}</div>
        <div className="trend-plot">
          <svg className="trend-svg" viewBox={`0 -8 ${WIDTH} ${HEIGHT + 16}`} preserveAspectRatio="none" role="img" aria-label={`${period} token ${mode === 'line' ? '曲线' : '柱状'}图；完整数值可悬停或聚焦数据点查看`}>
            <defs><linearGradient id={fillId} x1="0" x2="0" y1="0" y2="1"><stop offset="0%" stopColor="var(--accent)" stopOpacity=".24" /><stop offset="100%" stopColor="var(--accent)" stopOpacity=".01" /></linearGradient></defs>
            {axis.map(tick => <line className="chart-grid-line" key={tick.position} x1="0" x2={WIDTH} y1={tick.position / 100 * HEIGHT} y2={tick.position / 100 * HEIGHT} vectorEffect="non-scaling-stroke" />)}
            {mode === 'line' ? <>{paths.map((path, index) => <g key={index}><path d={`${path} L ${segments[index].at(-1)!.x} ${HEIGHT} L ${segments[index][0].x} ${HEIGHT} Z`} fill={`url(#${fillId})`} /><path d={path} className="chart-curve" vectorEffect="non-scaling-stroke" /></g>)}{points.filter(point => point.y !== null).map(point => <circle key={point.start} className="chart-dot-point" cx={point.x} cy={point.y!} r={active === point.start ? 5 : 2.6} vectorEffect="non-scaling-stroke" />)}</> : points.filter(point => point.y !== null).map(point => <rect key={point.start} className="chart-column" x={point.x - Math.min(28, WIDTH / Math.max(1, points.length) * .52) / 2} y={point.y!} width={Math.min(28, WIDTH / Math.max(1, points.length) * .52)} height={HEIGHT - point.y!} rx="3" />)}
            {selected && <line className="chart-crosshair" x1={selected.x} x2={selected.x} y1="0" y2={HEIGHT} vectorEffect="non-scaling-stroke" />}
          </svg>
          <div className="chart-hit-targets" role="group" aria-label="趋势数据点">{points.map(point => <button key={point.start} tabIndex={active === point.start || (selected === null && point.index === 0) ? 0 : -1} aria-describedby={active === point.start ? `${fillId}-tooltip` : undefined} aria-label={`${point.start}：${formatTokens(point.usage.tokens.total.value)} token`} style={{ left: `${point.x / WIDTH * 100}%`, width: `${Math.max(.8, 100 / Math.max(1, points.length))}%` }} onPointerEnter={() => setActive(point.start)} onFocus={() => setActive(point.start)} onBlur={() => setActive(null)} onClick={() => setActive(point.start)} onKeyDown={event => { if (event.key === 'Escape') setActive(null); if (event.key === 'ArrowLeft' || event.key === 'ArrowRight') { event.preventDefault(); const next = Math.max(0, Math.min(points.length - 1, point.index + (event.key === 'ArrowLeft' ? -1 : 1))); (event.currentTarget.parentElement?.children[next] as HTMLButtonElement)?.focus(); } }} />)}</div>
          <div className="trend-x-axis" aria-hidden="true">{ticks.map(point => <span key={point.start} style={{ left: `${point.x / WIDTH * 100}%` }}>{dateLabel(point.start)}</span>)}</div>
          {selected && <div className="chart-tooltip" role="tooltip" id={`${fillId}-tooltip`} style={{ left: `clamp(110px, ${selected.x / WIDTH * 100}%, calc(100% - 110px))` }}><span className="small muted">{selected.start} · {period}</span><strong>{formatTokens(selected.usage.tokens.total.value)} <span className="small muted">token</span></strong>{formatChineseMagnitude(selected.usage.tokens.total.value) && <span className="chart-readable">{formatChineseMagnitude(selected.usage.tokens.total.value)}</span>}<QualityBadge metric={selected.usage.tokens.total} /><span className="chart-tooltip-quality">{qualityDescription(selected.usage.tokens.total)}</span></div>}
        </div>
      </div>
      <p className="chart-hint">悬停或用方向键查看完整数值 · 未提供的数据留空</p>
    </>}
  </Panel>;
}
