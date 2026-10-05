import type { OverviewResult, ProviderSummary, UsageGroup } from '../api/generated/usage';
import { barPercent, formatTime, formatTokens, largestMetric } from '../components/format';
import { CoverageDetails, EmptyState, MetricCard, MetricValue, Notice, Panel, QualityBadge } from '../components/ui';

const warningLabels: Record<string, string> = {
  MODEL_BREAKDOWN_UNAVAILABLE: '部分来源缺少模型明细，模型分布不能代表全部用量。',
  MISSING_PRICING: '部分模型缺少价格，成本只包含已估价的部分。',
  SESSION_ACTIVITY_UNAVAILABLE: '部分会话缺少活动时间。',
};
export function WarningList({ warnings }: { warnings: string[] }) {
  const visible = warnings.filter(value => value !== 'DEMO_DATA');
  return visible.length ? <Notice tone="warning" title="数据说明"><ul>{visible.map(value => <li key={value}>{warningLabels[value] ?? `来源提示：${value}`}</li>)}</ul></Notice> : null;
}

function Distribution({ groups, label }: { groups: UsageGroup[]; label: (id: string) => string }) {
  const maximum = largestMetric(groups.map(group => group.usage.tokens.total.value));
  return groups.length ? <div className="distribution-list">{groups.map((group, index) => <div className="distribution-row" key={group.id}><div className="distribution-label"><span className={`chart-dot color-${index % 3}`} /><span>{label(group.id)}</span><span className="distribution-value"><MetricValue metric={group.usage.tokens.total} /></span></div><div className="bar-track" aria-hidden="true"><div className={`bar-fill color-${index % 3}`} style={{ width: `${barPercent(group.usage.tokens.total.value, maximum)}%` }} /></div><div className="distribution-meta"><QualityBadge metric={group.usage.tokens.total} /><span className="small muted">API 等价估算 <MetricValue metric={group.usage.cost.amountUsd} money /></span></div></div>)}</div> : <EmptyState title="没有可用的分布明细" description="来源未提供明细或所选范围没有数据。" />;
}

export function Overview({ result, providers, timezone }: { result: OverviewResult; providers: ProviderSummary[]; timezone: string }) {
  const usage = result.usage;
  const maximum = largestMetric(result.buckets.map(bucket => bucket.usage.tokens.total.value));
  const partial = result.coverage.some(coverage => coverage.state !== 'complete');
  return <div className="page-stack">
    {result.stale && <Notice tone="warning" title="正在显示上次成功的快照">数据已过期。扫描失败或来源不可读时，历史结果会保留；可在来源页面重新扫描。</Notice>}
    {partial && <Notice tone="warning" title="覆盖不完整">这里展示可获得的本机用量。来源报告的精确计数不代表覆盖全部活动。</Notice>}
    <div className="metrics-grid">
      <MetricCard label="已知总 token" metric={usage.tokens.total} note="由来源归一化后的总量" accent />
      <MetricCard label="未缓存输入" metric={usage.tokens.inputUncached} note="与缓存读取 / 写入互斥" />
      <MetricCard label="输出 token" metric={usage.tokens.outputTotal} note="已包含可获得的推理输出" />
      <MetricCard label="API 等价估算成本" metric={usage.cost.amountUsd} note="USD · 不代表订阅账单" money />
    </div>
    <Panel title="用量趋势" eyebrow="ACTIVITY" action={<span className="small muted">{result.query.bucket === 'day' ? '每日' : result.query.bucket === 'week' ? 'ISO 周' : '每月'} · token</span>}>
      {result.buckets.length ? <div className="trend-chart" role="img" aria-label="所选日期范围内的已知 token 趋势">{result.buckets.map(bucket => <div className="trend-column" key={bucket.start}><span className="trend-value">{formatTokens(bucket.usage.tokens.total.value)}</span><div className="trend-track"><div className="trend-bar" style={{ height: `${barPercent(bucket.usage.tokens.total.value, maximum)}%` }} /></div><span className="trend-label">{bucket.start.slice(5)}</span></div>)}</div> : <EmptyState title={usage.tokens.total.value === null ? '尚未采集用量' : '所选范围暂无记录'} description={usage.tokens.total.value === null ? '未知值显示为不可用。请在来源页面检测并扫描已启用的来源。' : '零值仅表示已知范围没有记录；其他未提供字段仍显示为不可用。'} />}
    </Panel>
    <div className="two-columns">
      <Panel title="来源分布" eyebrow="PROVIDERS" action={<span className="pill">{result.byProvider.length} 个来源</span>}><Distribution groups={result.byProvider} label={id => providers.find(provider => provider.providerId === id)?.displayName ?? id} /></Panel>
      <Panel title="模型分布" eyebrow="MODELS" action={<span className="pill">已提供的明细</span>}><Distribution groups={result.byModel} label={id => id} /><p className="panel-footnote">模型明细与总计是同一用量的不同视图，不重复相加。</p></Panel>
    </div>
    <WarningList warnings={result.warnings} />
    <Panel title="字段精度与统计覆盖" eyebrow="DATA QUALITY">
      <div className="quality-grid">{([
        ['未缓存输入', usage.tokens.inputUncached], ['缓存读取', usage.tokens.cacheRead], ['缓存写入', usage.tokens.cacheWrite],
        ['输出（含推理）', usage.tokens.outputTotal], ['推理输出（输出子集）', usage.tokens.outputReasoning],
      ] as const).map(([label, metric]) => <div className="quality-item" key={label}><span className="muted">{label}</span><MetricValue metric={metric} /><QualityBadge metric={metric} /></div>)}</div>
      <div className="metadata-grid"><div><h3>快照覆盖范围</h3><CoverageDetails coverage={result.coverage} /></div><div><h3>估价依据</h3><p className="small muted">价格版本：{usage.cost.pricingVersions.length ? usage.cost.pricingVersions.join('、') : '未知'}</p><p className="small muted">价格日期：{usage.cost.pricingAsOf ?? '未知'}</p>{usage.cost.missingModels.length > 0 && <p className="small warning-text">未估价模型：{usage.cost.missingModels.join('、')}</p>}</div></div>
      <div className="panel-footer"><span>最后成功更新 {formatTime(result.lastSuccessAt, timezone)}</span><span>{timezone} · 来源精度与覆盖范围分别标注</span></div>
    </Panel>
  </div>;
}
