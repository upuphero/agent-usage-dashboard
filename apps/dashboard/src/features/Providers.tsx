import type { ProviderSummary } from '../api/generated/usage';
import { Icon } from '../components/Icon';
import { formatTime } from '../components/format';
import { CoverageDetails, EmptyState, Panel, StatusBadge } from '../components/ui';

export function Providers({ providers, timezone, canScan, busy, onScan }: { providers: ProviderSummary[]; timezone: string; canScan: boolean; busy: boolean; onScan: (id: string) => void }) {
  if (!providers.length) return <Panel title="本机数据来源"><EmptyState title="尚未连接数据来源" description="来源检测和采集服务就绪后会在此显示。当前没有已连接的来源。" /></Panel>;
  return <div className="page-stack"><div className="provider-grid">{providers.map((provider, index) => <article className="panel provider-card" key={provider.providerId}>
    <div className="provider-header"><div className={`provider-mark color-${index % 3}`}>{provider.displayName.slice(0, 1)}</div><div><h2>{provider.displayName}</h2><p className="small muted">{provider.productId}</p></div><StatusBadge state={provider.state} /></div>
    <div className="provider-path"><Icon name="providers" /><span>{provider.pathHint ?? '来源位置不可用'}</span></div>
    <dl className="provider-details"><div><dt>最后成功扫描</dt><dd>{formatTime(provider.lastSuccessAt, timezone)}</dd></div><div><dt>采集方式</dt><dd>{provider.capabilities.supportsIncrementalCollection ? '增量采集' : '完整扫描'}</dd></div><div><dt>可用报表</dt><dd>{provider.capabilities.reportKinds.map(kind => kind === 'daily' ? '每日' : '会话').join('、') || '暂无'}</dd></div></dl>
    <div className="capability-tags"><span className="small muted">支持维度</span>{provider.capabilities.supportedDimensions.map(dimension => <span className="pill" key={dimension}>{({ day: '日期', model: '模型', session: '会话', project: '项目' } as Record<string, string>)[dimension] ?? dimension}</span>)}</div>
    <CoverageDetails coverage={provider.coverage} />
    {provider.lastScan && <div className="provider-last-scan"><StatusBadge state={provider.lastScan.state} />{provider.lastScan.error && <p className="small warning-text">{provider.lastScan.error.code} · {provider.lastScan.error.message}</p>}</div>}
    <div className="provider-actions"><span className="small muted">{provider.enabled ? '已启用' : '已关闭'}{!provider.capabilities.supportsQuota ? ' · 额度不可用' : ''}</span><button className="button button-small" disabled={!canScan || !provider.enabled || busy || provider.state === 'scanning'} onClick={() => onScan(provider.providerId)}><Icon name="refresh" />{provider.state === 'scanning' ? '扫描中' : '扫描来源'}</button></div>
  </article>)}</div><Panel title="统计范围"><p className="muted">每个来源独立声明支持的报表、维度和字段。不可用的筛选会禁用；来源不可读、格式不兼容或扫描失败时，保留上次成功的数据。</p><p className="panel-footnote">本机日志中的用量不代表账号所有云端活动。Codex、Claude Code 与网页产品分别统计。</p></Panel></div>;
}
