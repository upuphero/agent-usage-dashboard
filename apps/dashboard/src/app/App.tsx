import { useEffect, useState } from 'react';
import { useQueryClient } from '@tanstack/react-query';
import type { Bucket, ExportFormat, OverviewQuery, SessionQuery } from '../api/generated/usage';
import { normalizeError } from '../api/protocol';
import { Icon, type IconName } from '../components/Icon';
import { ErrorNotice, LoadingState, Notice, Panel, EmptyState, StatusBadge } from '../components/ui';
import { displayRange, rangeForPreset, todayInTimezone, type DatePreset } from '../features/dates';
import { Overview } from '../features/Overview';
import { Providers } from '../features/Providers';
import { Sessions } from '../features/Sessions';
import { Settings, type Theme } from '../features/Settings';
import { useApiInfo, useExport, useOverview, useProviders, useScan, useSessions, useSettings } from '../features/useUsage';

const pages = {
  overview: { title: '用量概览', english: 'Overview', description: '看清 Agent 的用量、来源与数据覆盖。', icon: 'overview' },
  providers: { title: '数据来源', english: 'Providers', description: '管理本机来源，查看检测结果与扫描状态。', icon: 'providers' },
  sessions: { title: '会话记录', english: 'Sessions', description: '浏览会话累计用量与可获得的元数据。', icon: 'sessions' },
  settings: { title: '设置', english: 'Settings', description: '显示偏好、统计时区与本地数据。', icon: 'settings' },
} as const;
type Page = keyof typeof pages;
function readPage(): Page {
  const value = typeof window === 'undefined' ? '' : window.location.hash.slice(1);
  return Object.hasOwn(pages, value) ? value as Page : 'overview';
}
function readTheme(): Theme {
  try { const value = localStorage.getItem('usage-dashboard-theme'); if (value === 'light' || value === 'system') return value; } catch { /* local preference storage can be unavailable */ }
  return 'dark';
}
const scenarios = [
  ['partial', '部分覆盖'], ['stale', '过期快照'], ['empty', '空数据'], ['error', '读取错误'],
  ['loading', '加载中'], ['scan-error', '扫描失败'], ['limited', '能力受限'], ['version-mismatch', '版本不兼容'],
] as const;

export function App({ demo, scenario = 'partial' }: { demo: boolean; scenario?: string }) {
  const cache = useQueryClient();
  const [page, setPage] = useState<Page>(readPage);
  const [theme, setTheme] = useState<Theme>(readTheme);
  const [timezone, setTimezone] = useState('America/Phoenix');
  const [preset, setPreset] = useState<DatePreset>('week');
  const [bucket, setBucket] = useState<Bucket>('day');
  const [providerId, setProviderId] = useState('');
  const [modelId, setModelId] = useState('');
  const [activeOnly, setActiveOnly] = useState(false);
  const [offset, setOffset] = useState(0);
  const [previousOffsets, setPreviousOffsets] = useState<number[]>([]);
  const resetPagination = () => { setOffset(0); setPreviousOffsets([]); };

  useEffect(() => {
    const changed = () => { const value = window.location.hash.slice(1); if (Object.hasOwn(pages, value)) setPage(value as Page); };
    window.addEventListener('hashchange', changed);
    return () => window.removeEventListener('hashchange', changed);
  }, []);
  useEffect(() => {
    const system = window.matchMedia('(prefers-color-scheme: dark)');
    const apply = () => { document.documentElement.dataset.theme = theme === 'system' ? system.matches ? 'dark' : 'light' : theme; };
    apply(); system.addEventListener('change', apply);
    try { localStorage.setItem('usage-dashboard-theme', theme); } catch { /* session-only theme remains functional */ }
    return () => system.removeEventListener('change', apply);
  }, [theme]);

  const api = useApiInfo();
  const settings = useSettings(api.isSuccess && api.data.capabilities.includes('settings-read'));
  const savedTimezone = settings.query.data?.timezone;
  useEffect(() => { if (savedTimezone) { setTimezone(savedTimezone); setOffset(0); setPreviousOffsets([]); } }, [savedTimezone]);
  const providersQuery = useProviders(api.isSuccess);
  const providers = providersQuery.data ?? [];
  const selected = providerId ? providers.filter(provider => provider.providerId === providerId) : providers;
  const sessionProviders = selected.filter(provider => provider.capabilities.reportKinds.includes('session'));
  const effective = page === 'sessions' ? sessionProviders : selected;
  const modelSupported = effective.length > 0 && effective.every(provider => provider.capabilities.supportedDimensions.includes('model'));
  const overviewModelSupported = selected.length > 0 && selected.every(provider => provider.capabilities.supportedDimensions.includes('model'));
  const overviewSupported = selected.length === 0 || selected.every(provider => provider.capabilities.reportKinds.includes('daily') && provider.capabilities.supportedDimensions.includes('day'));
  const range = rangeForPreset(demo ? '2026-10-04' : todayInTimezone(timezone), preset);
  const query: OverviewQuery = { range, timezone, bucket, providerIds: providerId ? [providerId] : [], modelIds: overviewModelSupported && modelId ? [modelId] : [] };
  const modelOptions = useOverview({ ...query, modelIds: [] }, api.isSuccess && providersQuery.isSuccess && overviewSupported && !!api.data.capabilities.includes('overview'));
  const overview = useOverview(query, api.isSuccess && providersQuery.isSuccess && overviewSupported && !!api.data.capabilities.includes('overview'));
  const sessionQuery: SessionQuery = { timezone, providerIds: sessionProviders.map(provider => provider.providerId), modelIds: modelSupported && modelId ? [modelId] : [], activeRange: activeOnly ? range : null, offset, limit: 20 };
  const sessions = useSessions(sessionQuery, page === 'sessions' && api.isSuccess && providersQuery.isSuccess && sessionProviders.length > 0 && !!api.data.capabilities.includes('sessions'));
  const scan = useScan(timezone);
  const exporting = useExport();
  const availableModels = modelOptions.data?.byModel ?? [];
  // Keep the selected label present while a new range/source response is loading.
  const models = modelId && !availableModels.some(model => model.id === modelId) ? [...availableModels, { id: modelId }] : availableModels;
  const changeProvider = (value: string) => { setProviderId(value); setModelId(''); resetPagination(); };
  const refresh = () => { void cache.invalidateQueries({ queryKey: ['usage'], predicate: value => value.queryKey[1] !== 'api' }); };
  const doExport = (format: ExportFormat) => exporting.mutate({ format, query });
  const selectScenario = (value: string) => { const url = new URL(window.location.href); url.searchParams.set('scenario', value); window.location.assign(url.href); };
  const info = pages[page];

  return <div className="app-layout">
    <a className="skip-link" href="#main-content">跳到主要内容</a>
    <aside className="sidebar"><a href="#overview" className="brand"><span className="brand-logo"><Icon name="bolt" /></span><span>Agent Usage<span className="brand-caption">LOCAL DASHBOARD</span></span></a><p className="nav-caption">工作空间</p><nav aria-label="主导航">{Object.entries(pages).map(([key, value]) => <a className={`nav-item ${page === key ? 'active' : ''}`} aria-label={value.english} aria-current={page === key ? 'page' : undefined} href={`#${key}`} key={key}><Icon name={value.icon as IconName} /><span>{value.english}</span>{page === key && <span className="nav-active-dot" />}</a>)}</nav><div className="sidebar-bottom"><div className="local-card"><Icon name="shield" /><span>{demo ? '合成演示数据' : '本地工作空间'}<span className="small muted">{demo ? '未读取本机日志' : '本机数据 · 默认离线'}</span></span></div><div className="sidebar-version"><span>v{api.data?.appVersion ?? '0.0.1'}</span><span>{demo ? 'DEMO' : 'DESKTOP'}</span></div></div></aside>
    <div className="main-shell"><header className="topbar"><div className="breadcrumb">工作空间 <span>/</span> {info.english}</div><div className="topbar-right"><span className="platform-label">Windows x64 · Apple Silicon</span><button className="icon-button" aria-label="切换深浅主题" title="切换深浅主题" onClick={() => setTheme(theme === 'light' ? 'dark' : 'light')}><Icon name="sun" /></button></div></header>
    <main id="main-content" className="main-content" tabIndex={-1}>
      {demo && <div className="demo-banner"><span className="demo-label">DEMO</span><div><strong>演示模式 · 合成数据</strong><p>示例日期固定为 2026-09-28 至 2026-10-04；扫描和导出均为演示。</p></div><label className="demo-select-label">演示场景<select aria-label="演示场景" value={scenario} onChange={event => selectScenario(event.target.value)}>{scenarios.map(([value, label]) => <option value={value} key={value}>{label}</option>)}</select></label></div>}
      <div className="page-heading"><div><p className="eyebrow">{info.english.toUpperCase()}</p><h1>{info.title}</h1><p className="muted">{info.description}</p></div><button className="button" disabled={!api.isSuccess || providersQuery.isFetching} onClick={refresh}><Icon name="refresh" className={providersQuery.isFetching ? 'spin' : ''} />刷新视图</button></div>
      {api.isPending ? <LoadingState label="正在连接用量服务…" /> : api.isError ? <ErrorNotice error={normalizeError(api.error)} retry={() => { void api.refetch(); }} /> : <>
        {api.data.capabilities.includes('adapter-integration-pending') && <Notice tone="warning" title="采集服务尚未接入">桌面接口已响应，真实来源仍待接入。当前页面没有真实采集结果。</Notice>}
        <div className="filter-panel"><div className="filter-field"><label htmlFor="date-preset">日期范围</label><select id="date-preset" value={preset} onChange={event => { setPreset(event.target.value as DatePreset); resetPagination(); }}><option value="today">今日</option><option value="week">本周 · ISO 周一开始</option><option value="month">本月</option></select></div><div className="filter-field"><label htmlFor="provider-filter">来源</label><select id="provider-filter" value={providerId} disabled={providersQuery.isPending} onChange={event => changeProvider(event.target.value)}><option value="">全部来源</option>{providers.map(provider => <option value={provider.providerId} key={provider.providerId}>{provider.displayName}</option>)}</select></div><div className="filter-field"><label htmlFor="model-filter">模型</label><select id="model-filter" value={modelSupported ? modelId : ''} disabled={!modelSupported} title={!modelSupported ? '所选来源不支持模型筛选' : undefined} onChange={event => { setModelId(event.target.value); resetPagination(); }}><option value="">全部模型</option>{models.map(model => <option value={model.id} key={model.id}>{model.id}</option>)}</select></div><div className="filter-field"><label htmlFor="bucket-filter">趋势粒度</label><select id="bucket-filter" value={bucket} disabled={page === 'sessions' || !overviewSupported} onChange={event => setBucket(event.target.value as Bucket)}><option value="day">每日</option><option value="week">每周</option><option value="month">每月</option></select></div><div className="filter-summary"><Icon name="clock" /><span>{displayRange(range)} <span className="muted">· {timezone}</span></span>{!modelSupported && <span className="filter-hint">所选来源不支持模型筛选</span>}</div></div>
        {scan.pending || scan.job ? <Notice title={scan.job ? '扫描任务进行中' : '正在启动扫描'} action={<div className="button-group">{scan.job && !scan.pending && <button className="button button-small" onClick={scan.resume}>继续等待</button>}<button className="button button-small" disabled={!scan.job || scan.cancelling} onClick={scan.cancel}>{scan.cancelling ? '正在取消…' : '取消扫描'}</button></div>}>扫描期间可继续浏览已有数据。{scan.job && <span className="small muted">任务 {scan.job.jobId}</span>}</Notice> : scan.outcome && <Notice tone={scan.outcome.state === 'failed' ? 'warning' : 'success'} title={demo ? '演示扫描结果' : '扫描结果'}><StatusBadge state={scan.outcome.state} />{scan.outcome.state === 'failed' ? ' 上次成功数据已保留。' : scan.outcome.state === 'cancelled' ? ' 已取消，已有快照未清空。' : demo ? ' 示例数据已刷新。' : ' 已更新用量快照。'}</Notice>}
        {scan.error && <ErrorNotice error={scan.error} />}
        {exporting.isPending && <Notice>正在准备导出…</Notice>}
        {exporting.isError && (normalizeError(exporting.error).code === 'CANCELLED' ? <Notice>已取消保存。</Notice> : <ErrorNotice error={normalizeError(exporting.error)} />)}
        {exporting.data && <Notice tone="success">{demo ? '演示导出请求完成，未保存文件。' : `已保存 ${exporting.data.suggestedFilename}。`}</Notice>}
        {providersQuery.isError && <ErrorNotice error={normalizeError(providersQuery.error)} retry={() => { void providersQuery.refetch(); }} />}
        {providersQuery.isPending ? <LoadingState label="正在加载来源信息…" /> : <>
          {page === 'overview' && <>{overview.isError && <ErrorNotice error={normalizeError(overview.error)} retry={() => { void overview.refetch(); }} />}{overview.data ? <Overview result={overview.data} providers={providers} timezone={timezone} /> : overview.isLoading ? <LoadingState /> : (!overviewSupported || !api.data.capabilities.includes('overview')) && <Panel title="用量概览"><EmptyState title="此服务暂不支持概览" /></Panel>}</>}
          {page === 'providers' && <Providers providers={providers} timezone={timezone} canScan={api.data.capabilities.includes('scan-polling')} busy={scan.pending || !!scan.job} onScan={scan.start} />}
          {page === 'sessions' && <>{sessions.isError && <ErrorNotice error={normalizeError(sessions.error)} retry={() => { void sessions.refetch(); }} />}{sessions.isLoading && <LoadingState label="正在加载会话数据…" />}{!providerId && sessionProviders.length < selected.length && <Notice>仅列出支持会话报表的来源。其余来源的每日用量仍可在概览查看。</Notice>}<Sessions result={sessions.data} providers={providers} timezone={timezone} activeOnly={activeOnly} onActiveOnly={value => { setActiveOnly(value); resetPagination(); }} offset={offset} supported={sessionProviders.length > 0 && api.data.capabilities.includes('sessions')} onNext={() => { if (sessions.data?.nextOffset !== null && sessions.data?.nextOffset !== undefined) { setPreviousOffsets([...previousOffsets, offset]); setOffset(sessions.data.nextOffset); } }} onPrevious={() => { setOffset(previousOffsets.at(-1) ?? 0); setPreviousOffsets(previousOffsets.slice(0, -1)); }} /></>}
          {page === 'settings' && <Settings key={settings.query.data?.revision ?? 'unavailable'} theme={theme} setTheme={setTheme} timezone={timezone} setTimezone={value => { setTimezone(value); resetPagination(); }} demo={demo} providers={providers} api={api.data} modelFiltered={query.modelIds.length > 0} exporting={exporting.isPending} onExport={doExport}
            settingsLoading={api.data.capabilities.includes('settings-read') && settings.canRead && settings.query.isPending}
            settingsError={settings.query.error ? normalizeError(settings.query.error) : undefined}
            controls={settings.query.data && settings.canWrite && api.data.capabilities.includes('settings-write') ? { value: settings.query.data, pending: settings.save.isPending, choosing: settings.choose.isPending, busy: scan.pending || !!scan.job, saved: settings.save.isSuccess,
              error: settings.save.error ? normalizeError(settings.save.error) : settings.choose.error ? normalizeError(settings.choose.error) : undefined,
              save: request => settings.save.mutate(request), choose: settings.canChoose && api.data.capabilities.includes('source-directory-selection') ? async providerId => (await settings.choose.mutateAsync(providerId)).directory : undefined,
              reload: () => { settings.save.reset(); settings.choose.reset(); void settings.query.refetch(); },
            } : undefined} />}
        </>}
      </>}
      <footer className="app-footer"><span>Agent Usage Dashboard <span className="footer-dot">·</span> 本地优先</span><span>{demo ? '所有用量与价格均为合成示例' : 'API 等价估算不代表实际账单'}</span></footer>
    </main></div>
  </div>;
}
