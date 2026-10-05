import type { ReactNode } from 'react';
import type { ApiError, Coverage, Metric, ProviderState, ScanState } from '../api/generated/usage';
import { accuracyLabels, formatChineseMagnitude, formatTokens, formatUsd, qualityDescription } from './format';
import { Icon } from './Icon';
import { useI18n } from '../i18n/I18nContext';
import { localizeError, type MessageKey } from '../i18n/messages';

const stateLabels: Record<ProviderState | ScanState, MessageKey> = {
  'not-detected': '未检测到', 'no-data': '暂无数据', ready: '就绪', scanning: '扫描中',
  'permission-denied': '权限不足', 'schema-unsupported': '格式不支持', partial: '部分覆盖', stale: '数据过期', error: '来源错误',
  queued: '排队中', running: '扫描中', succeeded: '扫描完成', failed: '扫描失败', cancelled: '已取消',
};
export function StatusBadge({ state }: { state: ProviderState | ScanState }) {
  const { t } = useI18n();
  return <span className={`badge state-${state}`}><span className="status-dot" />{t(stateLabels[state])}</span>;
}
export function QualityBadge({ metric }: { metric: Metric<string> }) {
  const { t, language } = useI18n();
  return <span className={`quality quality-${metric.accuracy}`} title={qualityDescription(metric, language)}>{metric.missingRows && metric.value !== null ? t('已知部分 · ') : ''}{t(accuracyLabels[metric.accuracy])}</span>;
}
export function MetricValue({ metric, money = false }: { metric: Metric<string>; money?: boolean }) {
  const { language } = useI18n();
  return <span className={metric.value === null ? 'unavailable' : 'numeric'} title={qualityDescription(metric, language)}>{money ? formatUsd(metric.value, language) : formatTokens(metric.value, language)}</span>;
}
export function MetricCard({ label, metric, note, money = false, accent = false, locale }: { label: string; metric: Metric<string>; note: string; money?: boolean; accent?: boolean; locale?: string }) {
  const { t, locale: uiLocale } = useI18n();
  const reading = money ? null : formatChineseMagnitude(metric.value, locale ?? uiLocale);
  return <article className={`metric-card ${accent ? 'metric-accent' : ''}`}><div className="metric-label">{label}<Icon name={money ? 'info' : 'bolt'} /></div><div className="metric-number"><MetricValue metric={metric} money={money} /></div>{reading && <p className="metric-readable" lang="zh-CN">{reading}</p>}<div className="metric-bottom"><QualityBadge metric={metric} /><span className="small muted">{note}</span></div>{metric.missingRows > 0 && metric.value !== null && <p className="small warning-text">{t('{count} 条记录缺失 · 仅已知部分', { count: metric.missingRows })}</p>}</article>;
}
export function Panel({ title, eyebrow, children, action, className = '' }: { title: string; eyebrow?: string; children: ReactNode; action?: ReactNode; className?: string }) {
  return <section className={`panel ${className}`}><div className="panel-header"><div>{eyebrow && <p className="eyebrow">{eyebrow}</p>}<h2>{title}</h2></div>{action}</div>{children}</section>;
}
export function Notice({ children, tone = 'info', title, action }: { children: ReactNode; tone?: 'info' | 'warning' | 'error' | 'success'; title?: string; action?: ReactNode }) {
  return <div className={`notice notice-${tone}`} role={tone === 'error' ? 'alert' : 'status'}><Icon name={tone === 'success' ? 'check' : 'info'} /><div>{title && <strong>{title}</strong>}<div>{children}</div></div>{action}</div>;
}
export function ErrorNotice({ error, retry }: { error: ApiError; retry?: () => void }) {
  const { t, language } = useI18n();
  return <Notice tone="error" title={t(error.code === 'API_VERSION_UNSUPPORTED' ? '接口版本不兼容' : '请求未完成')} action={retry && error.retryable ? <button className="button button-small" onClick={retry}>{t('重试')}</button> : undefined}><p>{localizeError(error, language)}</p><code className="small">{error.code}</code></Notice>;
}
export function LoadingState({ label }: { label?: string }) {
  const { t } = useI18n();
  return <div className="loading-state" role="status" aria-busy="true"><div className="spinner" /><p>{label ?? t('正在加载用量数据…')}</p><div className="skeleton-grid">{[0, 1, 2].map(i => <div className="skeleton" key={i} />)}</div></div>;
}
export function EmptyState({ title, description, action }: { title?: string; description?: string; action?: ReactNode }) {
  const { t } = useI18n();
  return <div className="empty-state"><span className="empty-icon"><Icon name="sessions" /></span><h3>{title ?? t('暂无用量数据')}</h3><p>{description ?? t('当前范围内没有数据。可调整筛选或扫描已启用的来源。')}</p>{action}</div>;
}
export function CoverageDetails({ coverage }: { coverage: Coverage[] }) {
  const { t } = useI18n();
  if (coverage.length === 0) return <span className="small muted">{t('覆盖范围尚未提供')}</span>;
  return <div className="coverage-list">{coverage.map((item, index) => <span className="small" key={index}><span className={`coverage-dot ${item.state}`} />{t(item.state === 'complete' ? '完整快照' : item.state === 'partial' ? '部分日志' : '历史归档')}{item.range ? t(' · {start} ≤ 日期 < {end}', { start: item.range.start, end: item.range.end }) : t(' · 日期范围未知')}</span>)}</div>;
}
