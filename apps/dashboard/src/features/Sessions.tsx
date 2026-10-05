import type { ProviderSummary, SessionPage } from '../api/generated/usage';
import { formatTime } from '../components/format';
import { EmptyState, MetricValue, Notice, Panel, QualityBadge } from '../components/ui';
import { WarningList } from './Overview';

export function Sessions({ result, providers, timezone, activeOnly, onActiveOnly, offset, onNext, onPrevious, supported }: { result?: SessionPage; providers: ProviderSummary[]; timezone: string; activeOnly: boolean; onActiveOnly: (value: boolean) => void; offset: number; onNext: () => void; onPrevious: () => void; supported: boolean }) {
  return <div className="page-stack">
    <Notice title="会话累计用量">日期筛选仅查找期间最后活跃的会话，表格始终展示全会话累计量；不会把累计 token 归入最后活动的某一天。</Notice>
    <div className="session-options"><label className="checkbox-label"><input type="checkbox" checked={activeOnly} disabled={!supported} onChange={event => onActiveOnly(event.target.checked)} />仅显示所选期间活跃的会话</label><label className="checkbox-label disabled" title="来源不支持日期与会话用量交叉查询"><input type="checkbox" disabled />仅统计所选期间的会话 token</label></div>
    {!supported ? <Panel title="会话报表"><EmptyState title="此来源不支持会话报表" description="请选择支持会话能力的来源；不支持的报表不会返回伪造的零值。" /></Panel> : result && <>
      {result.stale && <Notice tone="warning" title="会话快照已过期">正在显示上次成功的数据，可在来源页面重新扫描。</Notice>}
      <WarningList warnings={result.warnings} />
      {result.dateFilterSemantics !== 'active-sessions-lifetime-usage' && <Notice tone="warning">会话日期语义尚未识别：{result.dateFilterSemantics}。请在确认统计口径后使用日期筛选。</Notice>}
      <Panel title="会话列表" eyebrow="SESSIONS" action={<span className="pill">{result.total} 个会话</span>}>
        {result.items.length ? <div className="table-scroll"><table className="sessions-table"><thead><tr><th>会话 / 来源</th><th>模型</th><th>累计 token</th><th>API 等价估算成本</th><th>最后活动</th><th>元数据</th></tr></thead><tbody>{result.items.map(session => <tr key={`${session.sourceDatasetId}/${session.sessionId}/${session.modelId ?? ''}`}><td><strong className="session-id">{session.sessionId}</strong><span className="cell-subtitle">{providers.find(provider => provider.providerId === session.providerId)?.displayName ?? session.providerId}</span></td><td><span className="model-label">{session.modelId ?? '模型未知'}</span><span className="cell-subtitle">{session.modelVendor ?? '厂商未知'}</span></td><td><strong><MetricValue metric={session.usage.tokens.total} /></strong><span className="cell-subtitle"><QualityBadge metric={session.usage.tokens.total} /></span></td><td><MetricValue metric={session.usage.cost.amountUsd} money /><span className="cell-subtitle">USD · 估算</span></td><td><span className="small">{formatTime(session.lastActivityAt, timezone)}</span></td><td><details><summary>查看</summary><dl className="session-metadata"><dt>开始时间</dt><dd>{session.startedAt ? formatTime(session.startedAt, timezone) : '来源未提供'}</dd><dt>来源设备</dt><dd>{session.originDeviceId}</dd><dt>数据集</dt><dd>{session.sourceDatasetId}</dd><dt>未缓存输入</dt><dd><MetricValue metric={session.usage.tokens.inputUncached} /></dd><dt>输出（含推理）</dt><dd><MetricValue metric={session.usage.tokens.outputTotal} /></dd></dl></details></td></tr>)}</tbody></table></div> : <EmptyState title="没有符合筛选条件的会话" description="可调整来源、模型或活动日期筛选；该列表不代表期间用量为零。" />}
        <div className="panel-footer"><span>{result.items.length ? `第 ${offset + 1} — ${offset + result.items.length} 项` : '暂无记录'} · {timezone}</span><div className="button-group"><button className="button button-small" disabled={offset === 0} onClick={onPrevious}>上一页</button><button className="button button-small" disabled={result.nextOffset === null} onClick={onNext}>下一页</button></div></div>
      </Panel>
    </>}
  </div>;
}
