import { useState } from 'react';
import type { UsageGroup } from '../api/generated/usage';
import { compareDecimalDescending, decimalBarPercent } from '../components/format';
import { EmptyState, MetricValue, Panel, QualityBadge } from '../components/ui';
import { useI18n } from '../i18n/I18nContext';

export type ModelSort = 'tokens' | 'cost';
const positive = (value: string | null) => value !== null && /[1-9]/.test(value);
const valueFor = (group: UsageGroup, sort: ModelSort) => sort === 'tokens' ? group.usage.tokens.total : group.usage.cost.amountUsd;
export function partitionModels(groups: UsageGroup[], sort: ModelSort) {
  const ranked: UsageGroup[] = []; const unavailable: UsageGroup[] = []; const unused: UsageGroup[] = [];
  for (const group of groups) {
    const hasUsage = Object.values(group.usage.tokens).some(metric => positive(metric.value)) || positive(group.usage.cost.amountUsd.value);
    if (group.usage.tokens.total.value === '0' && !hasUsage) unused.push(group);
    else if (valueFor(group, sort).value === null) unavailable.push(group);
    else ranked.push(group);
  }
  const order = (a: UsageGroup, b: UsageGroup) => compareDecimalDescending(valueFor(a, sort).value, valueFor(b, sort).value) || a.id.localeCompare(b.id);
  ranked.sort(order); unavailable.sort((a, b) => compareDecimalDescending(valueFor(a, sort === 'tokens' ? 'cost' : 'tokens').value, valueFor(b, sort === 'tokens' ? 'cost' : 'tokens').value) || a.id.localeCompare(b.id));
  unused.sort((a, b) => a.id.localeCompare(b.id));
  return { ranked, unavailable, unused };
}
function ModelRows({ groups, sort }: { groups: UsageGroup[]; sort: ModelSort }) {
  const { t } = useI18n();
  const maximum = groups.reduce<string | null>((max, group) => compareDecimalDescending(valueFor(group, sort).value, max) < 0 ? valueFor(group, sort).value : max, null);
  return <div className="distribution-list">{groups.map(group => {
    const primary = valueFor(group, sort);
    const color = [...group.id].reduce((sum, letter) => sum + letter.charCodeAt(0), 0) % 3;
    return <div className="distribution-row" key={group.id}><div className="distribution-label"><span className={`chart-dot color-${color}`} /><span>{group.id}</span><span className="distribution-value"><MetricValue metric={primary} money={sort === 'cost'} /></span></div><div className="bar-track" aria-hidden="true"><div className={`bar-fill color-${color}`} style={{ width: `${decimalBarPercent(primary.value, maximum)}%` }} /></div><div className="distribution-meta"><QualityBadge metric={primary} /><span className="small muted">{sort === 'tokens' ? <>{t("API 等价估算")}{' '}<MetricValue metric={group.usage.cost.amountUsd} money /></> : <>token <MetricValue metric={group.usage.tokens.total} /></>}</span></div></div>;
  })}</div>;
}
export function ModelDistribution({ groups }: { groups: UsageGroup[] }) {
  const { t } = useI18n();
  const [sort, setSort] = useState<ModelSort>('tokens');
  const { ranked, unavailable, unused } = partitionModels(groups, sort);
  return <Panel title={t("模型分布")} eyebrow={t('模型明细')} className="model-panel" action={<select className="model-sort" aria-label={t("模型排序")} value={sort} onChange={event => setSort(event.target.value as ModelSort)}><option value="tokens">{t("按 token ↓")}</option><option value="cost">{t("按估算成本 ↓")}</option></select>}>
    {ranked.length > 0 ? <ModelRows groups={ranked} sort={sort} /> : groups.length === 0 ? <EmptyState title={t("所选范围没有模型用量")} /> : <p className="small muted">{t('本范围没有可按{metric}排序的完整明细，其他记录见下方。', { metric: sort === 'tokens' ? ' token' : t('成本') })}</p>}
    {unavailable.length > 0 && <details className="model-extra"><summary>{t('{metric}明细不足（{count} 个模型）', { metric: sort === 'tokens' ? 'token' : t('费用'), count: unavailable.length })}</summary><p className="small muted">{t("这些模型有记录，但当前字段未提供完整数值，不代表没有使用。")}</p><ModelRows groups={unavailable} sort={sort} /></details>}
    {unused.length > 0 && <details className="model-extra"><summary>{t('本范围未使用（{count} 个模型）', { count: unused.length })}</summary><ModelRows groups={unused} sort={sort} /></details>}
    <p className="panel-footnote">{t("从多到少排列；成本为 API 等价估算。模型明细与总计不重复相加。")}</p>
  </Panel>;
}
