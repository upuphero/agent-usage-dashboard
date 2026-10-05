import type { OverviewResult, ProviderSummary, UsageGroup } from '../api/generated/usage';
import { barPercent, formatTime, largestMetric } from '../components/format';
import { CoverageDetails, EmptyState, MetricCard, MetricValue, Notice, Panel, QualityBadge } from '../components/ui';
import { TrendChart } from './TrendChart';
import { ModelDistribution } from './ModelDistribution';
import { useI18n } from '../i18n/I18nContext';
import type { MessageKey } from '../i18n/messages';

const warningLabels: Record<string, MessageKey> = {
  MODEL_BREAKDOWN_UNAVAILABLE: '部分来源缺少模型明细，模型分布不能代表全部用量。',
  MISSING_PRICING: '部分模型缺少价格，成本只包含已估价的部分。',
  SESSION_ACTIVITY_UNAVAILABLE: '部分会话缺少活动时间。',
};
export function WarningList({ warnings }: { warnings: string[] }) {
  const { t } = useI18n();
  const visible = warnings.filter(value => value !== 'DEMO_DATA');
  return visible.length ? <Notice tone="warning" title={t("数据说明")}><ul>{visible.map(value => <li key={value}>{warningLabels[value] ? t(warningLabels[value]) : t('来源提示：{warning}', { warning: value })}</li>)}</ul></Notice> : null;
}

function Distribution({ groups, label }: { groups: UsageGroup[]; label: (id: string) => string }) {
  const { t } = useI18n();
  const maximum = largestMetric(groups.map(group => group.usage.tokens.total.value));
  return groups.length ? <div className="distribution-list">{groups.map((group, index) => <div className="distribution-row" key={group.id}><div className="distribution-label"><span className={`chart-dot color-${index % 3}`} /><span>{label(group.id)}</span><span className="distribution-value"><MetricValue metric={group.usage.tokens.total} /></span></div><div className="bar-track" aria-hidden="true"><div className={`bar-fill color-${index % 3}`} style={{ width: `${barPercent(group.usage.tokens.total.value, maximum)}%` }} /></div><div className="distribution-meta"><QualityBadge metric={group.usage.tokens.total} /><span className="small muted">{t("API 等价估算")}{' '}<MetricValue metric={group.usage.cost.amountUsd} money /></span></div></div>)}</div> : <EmptyState title={t("没有可用的分布明细")} description={t("来源未提供明细或所选范围没有数据。")} />;
}

export function Overview({ result, providers, timezone }: { result: OverviewResult; providers: ProviderSummary[]; timezone: string }) {
  const { t, language } = useI18n();
  const usage = result.usage;
  const partial = result.coverage.some(coverage => coverage.state !== 'complete');
  return <div className="page-stack">
    <TrendChart result={result} />
    {result.stale && <Notice tone="warning" title={t("正在显示上次成功的快照")}>{t("数据已过期。扫描失败或来源不可读时，历史结果会保留；可在来源页面重新扫描。")}</Notice>}
    {partial && <Notice tone="warning" title={t("覆盖不完整")}>{t("这里展示可获得的本机用量。来源报告的精确计数不代表覆盖全部活动。")}</Notice>}
    <div className="metrics-grid">
      <MetricCard label={t("已知总 token")} metric={usage.tokens.total} note={t("由来源归一化后的总量")} accent />
      <MetricCard label={t("未缓存输入")} metric={usage.tokens.inputUncached} note={t("与缓存读取 / 写入互斥")} />
      <MetricCard label={t("输出 token")} metric={usage.tokens.outputTotal} note={t("已包含可获得的推理输出")} />
      <MetricCard label={t("API 等价估算成本")} metric={usage.cost.amountUsd} note={t("USD · 不代表订阅账单")} money />
    </div>
    <div className="two-columns">
      <Panel title={t("来源分布")} eyebrow={t('来源明细')} action={<span className="pill">{t('{count} 个来源', { count: result.byProvider.length })}</span>}><Distribution groups={result.byProvider} label={id => providers.find(provider => provider.providerId === id)?.displayName ?? id} /></Panel>
      <ModelDistribution groups={result.byModel} />
    </div>
    <WarningList warnings={result.warnings} />
    <Panel title={t("字段精度与统计覆盖")} eyebrow={t('数据质量')}>
      <div className="quality-grid">{([
        [t("未缓存输入"), usage.tokens.inputUncached], [t("缓存读取"), usage.tokens.cacheRead], [t("缓存写入"), usage.tokens.cacheWrite],
        [t("输出（含推理）"), usage.tokens.outputTotal], [t("推理输出（输出子集）"), usage.tokens.outputReasoning],
      ] as const).map(([label, metric]) => <div className="quality-item" key={label}><span className="muted">{label}</span><MetricValue metric={metric} /><QualityBadge metric={metric} /></div>)}</div>
      <div className="metadata-grid"><div><h3>{t("快照覆盖范围")}</h3><CoverageDetails coverage={result.coverage} /></div><div><h3>{t("估价依据")}</h3><p className="small muted">{t('价格版本：{versions}', { versions: usage.cost.pricingVersions.length ? usage.cost.pricingVersions.join(language === 'zh' ? '、' : ', ') : t('未知') })}</p><p className="small muted">{t('价格日期：{date}', { date: usage.cost.pricingAsOf ?? t('未知') })}</p>{usage.cost.missingModels.length > 0 && <p className="small warning-text">{t('未估价模型：{models}', { models: usage.cost.missingModels.join(language === 'zh' ? '、' : ', ') })}</p>}</div></div>
      <div className="panel-footer"><span>{t('最后成功更新 {time}', { time: formatTime(result.lastSuccessAt, timezone, language) })}</span><span>{t('{timezone} · 来源精度与覆盖范围分别标注', { timezone })}</span></div>
    </Panel>
  </div>;
}
