import { useEffect, useState } from 'react';
import { useQueryClient } from '@tanstack/react-query';
import type { Bucket, ExportFormat, OverviewQuery, SessionQuery } from '../api/generated/usage';
import { normalizeError } from '../api/protocol';
import { Icon, type IconName } from '../components/Icon';
import { FilterPopover } from '../components/FilterPopover';
import { ErrorNotice, LoadingState, Notice, Panel, EmptyState, StatusBadge } from '../components/ui';
import { displayRange, rangeForPreset, systemTimezone, todayInTimezone, type DatePreset } from '../features/dates';
import { Overview } from '../features/Overview';
import { Providers } from '../features/Providers';
import { Sessions } from '../features/Sessions';
import { Settings, type Theme } from '../features/Settings';
import { TimezoneNotice } from '../features/TimezoneNotice';
import { useApiInfo, useExport, useOverview, useProviders, useScan, useSessions, useSettings, useAutoCollection, useBackgroundUsage, useTimezone } from '../features/useUsage';
import { useI18n } from '../i18n/I18nContext';

const pages = {
  overview: { title: '用量概览', nav: '概览', description: '看清 Agent 的用量、来源与数据覆盖。', icon: 'overview' },
  providers: { title: '数据来源', nav: '数据来源', description: '管理本机来源，查看检测结果与扫描状态。', icon: 'providers' },
  sessions: { title: '会话记录', nav: '会话记录', description: '浏览会话累计用量与可获得的元数据。', icon: 'sessions' },
  settings: { title: '设置', nav: '设置', description: '显示偏好、统计时区与本地数据。', icon: 'settings' },
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
  const { t, language, setLanguage } = useI18n();
  const cache = useQueryClient();
  const [page, setPage] = useState<Page>(readPage);
  const [theme, setTheme] = useState<Theme>(readTheme);
  const [legacyTimezone, setLegacyTimezone] = useState(() => demo ? 'America/Phoenix' : systemTimezone());
  const [preset, setPreset] = useState<DatePreset>('last30');
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
  useBackgroundUsage(api.isSuccess);
  const automatic = useAutoCollection(api.isSuccess && api.data.capabilities.includes('auto-full-scan'));
  const settings = useSettings(api.isSuccess && api.data.capabilities.includes('settings-read'));
  const savedTimezone = settings.query.data?.timezone;
  useEffect(() => { if (savedTimezone) { setLegacyTimezone(savedTimezone); setOffset(0); setPreviousOffsets([]); } }, [savedTimezone]);
  // API 1.3: the host's effective zone is the query/scan zone, derived during render; zone-dependent queries wait for it.
  const zone = useTimezone(api.isSuccess && api.data.capabilities.includes('timezone-follow-system'));
  const followsHost = api.isSuccess && api.data.capabilities.includes('timezone-follow-system') && zone.supported;
  const hostTimezone = followsHost ? zone.query.data?.effectiveTimezone : undefined;
  const timezone = hostTimezone ?? legacyTimezone;
  const timezoneReady = !followsHost || hostTimezone !== undefined;
  const [pagedTimezone, setPagedTimezone] = useState(timezone);
  if (pagedTimezone !== timezone) { setPagedTimezone(timezone); setOffset(0); setPreviousOffsets([]); }
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
  const overviewEnabled = api.isSuccess && providersQuery.isSuccess && timezoneReady && overviewSupported && !!api.data.capabilities.includes('overview');
  const modelOptions = useOverview({ ...query, modelIds: [] }, overviewEnabled);
  const overview = useOverview(query, overviewEnabled);
  const sessionQuery: SessionQuery = { timezone, providerIds: sessionProviders.map(provider => provider.providerId), modelIds: modelSupported && modelId ? [modelId] : [], activeRange: activeOnly ? range : null, offset, limit: 20 };
  const sessions = useSessions(sessionQuery, page === 'sessions' && api.isSuccess && providersQuery.isSuccess && timezoneReady && sessionProviders.length > 0 && !!api.data.capabilities.includes('sessions'));
  const timezoneLoading = !timezoneReady && zone.query.isPending;
  const scan = useScan(timezone);
  const backgroundScanning = !scan.pending && !scan.job && providers.some(provider => provider.state === 'scanning');
  const automaticJobs = automatic.query.data?.providers.filter(provider => provider.jobId) ?? [];
  const [enabling, setEnabling] = useState(false);
  const [setupError, setSetupError] = useState<unknown>(null);
  const enableAndScan = async (providerId: string) => {
    const current = settings.query.data;
    if (!current) return;
    setEnabling(true); setSetupError(null);
    try {
      await settings.save.mutateAsync({ expectedRevision: current.revision, timezone: current.timezone,
        providers: current.providers.map(provider => ({ providerId: provider.providerId, enabled: provider.providerId === providerId ? true : provider.enabled, directoryRef: provider.directory?.directoryRef ?? null })) });
      scan.start(providerId);
    } catch (error) { setSetupError(error); }
    finally { setEnabling(false); }
  };
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
    <a className="skip-link" href="#main-content">{t("跳到主要内容")}</a>
    <aside className="sidebar"><a href="#overview" className="brand"><span className="brand-logo"><Icon name="bolt" /></span><span>Agent Usage<span className="brand-caption">{t('本地看板')}</span></span></a><p className="nav-caption">{t("工作空间")}</p><nav aria-label={t("主导航")}>{Object.entries(pages).map(([key, value]) => <a className={`nav-item ${page === key ? 'active' : ''}`} aria-label={t(value.nav)} aria-current={page === key ? 'page' : undefined} href={`#${key}`} key={key}><Icon name={value.icon as IconName} /><span>{t(value.nav)}</span>{page === key && <span className="nav-active-dot" />}</a>)}</nav><div className="sidebar-bottom"><div className="local-card"><Icon name="shield" /><span>{demo ? t("合成演示数据") : t("本地工作空间")}<span className="small muted">{demo ? t("未读取本机日志") : t("本机数据 · 默认离线")}</span></span></div><div className="sidebar-version"><span>v{api.data?.appVersion ?? '0.0.1'}</span><span>{t(demo ? "演示" : "桌面版")}</span></div></div></aside>
    <div className="main-shell"><header className="topbar"><div className="breadcrumb">{t("工作空间")}{' '}<span>/</span> {t(info.nav)}</div><div className="topbar-right"><span className="platform-label">Windows x64 · Apple Silicon</span><div className="topbar-actions"><button className="icon-button" aria-label={t("切换深浅主题")} title={t("切换深浅主题")} onClick={() => setTheme(theme === 'light' ? 'dark' : 'light')}><Icon name="sun" /></button><button className="icon-button language-toggle" aria-label={t(language === 'zh' ? "切换到英文" : "切换到中文")} title={t(language === 'zh' ? "切换到英文" : "切换到中文")} onClick={() => setLanguage(language === 'zh' ? 'en' : 'zh')}><Icon name="language" /><span aria-hidden="true">{language === 'zh' ? '中' : 'EN'}</span></button></div></div></header>
    <main id="main-content" className="main-content" tabIndex={-1}>
      {demo && <div className="demo-banner"><span className="demo-label">{t('演示')}</span><div><strong>{t("演示模式 · 合成数据")}</strong><p>{t("所有数字为合成示例；扫描和导出均为演示，不读取本机日志。")}</p></div><label className="demo-select-label">{t("演示场景")}<select aria-label={t("演示场景")} value={scenario} onChange={event => selectScenario(event.target.value)}>{scenarios.map(([value, label]) => <option value={value} key={value}>{t(label)}</option>)}</select></label></div>}
      <div className="page-heading"><div><p className="eyebrow">{t(info.nav).toUpperCase()}</p><h1>{t(info.title)}</h1><p className="muted">{t(info.description)}</p></div><button className="button" disabled={!api.isSuccess || providersQuery.isFetching} onClick={refresh}><Icon name="refresh" className={providersQuery.isFetching ? 'spin' : ''} />{t("刷新视图")}</button></div>
      {api.isPending ? <LoadingState label={t("正在连接用量服务…")} /> : api.isError ? <ErrorNotice error={normalizeError(api.error)} retry={() => { void api.refetch(); }} /> : <>
        {api.data.capabilities.includes('adapter-integration-pending') && <Notice tone="warning" title={t("采集服务尚未接入")}>{t("桌面接口已响应，真实来源仍待接入。当前页面没有真实采集结果。")}</Notice>}
        {followsHost && <TimezoneNotice status={zone.query.data} names={Object.fromEntries(providers.map(provider => [provider.providerId, provider.displayName]))} cancelling={zone.cancel.isPending} onCancel={jobId => zone.cancel.mutate(jobId)} />}
        {followsHost && zone.query.isError && !zone.query.data && <ErrorNotice error={normalizeError(zone.query.error)} retry={() => { void zone.query.refetch(); }} />}{zone.cancel.isError && <ErrorNotice error={normalizeError(zone.cancel.error)} />}
        {backgroundScanning && automaticJobs.length === 0 && !(followsHost && zone.query.data?.rebuild === 'rebuilding') && <Notice title={t("正在更新用量统计")}>{t('正在按 {timezone} 重新统计已启用的来源，完成后会自动更新视图。', { timezone })}</Notice>}
        {!scan.job && automaticJobs.map(provider => <Notice key={provider.jobId} title={t('自动完整扫描进行中')} action={<button className="button button-small" disabled={automatic.cancel.isPending} onClick={() => automatic.cancel.mutate(provider.jobId!)}>{t('取消自动扫描')}</button>}>{providers.find(p => p.providerId === provider.providerId)?.displayName} · {t('扫描期间可继续浏览已有数据。')}</Notice>)}
        <div className="query-toolbar"><div className="query-summary"><Icon name="clock" /><span>{displayRange(range)} · {timezone}</span><span className="query-chip">{providerId ? providers.find(provider => provider.providerId === providerId)?.displayName : t("全部来源")}</span>{modelId && <span className="query-chip">{modelId}</span>}<span className="query-chip">{bucket === 'day' ? t("每日") : bucket === 'week' ? t("每周") : t("每月")}</span></div><FilterPopover summary={`${displayRange(range)} · ${providerId || t("全部来源")} · ${modelId || t("全部模型")}`}><div className="filter-panel"><div className="filter-field"><label htmlFor="date-preset">{t("日期范围")}</label><select id="date-preset" value={preset} onChange={event => { setPreset(event.target.value as DatePreset); resetPagination(); }}><option value="last30">{t("最近 30 天")}</option><option value="today">{t("今日")}</option><option value="week">{t("本周 · ISO 周一开始")}</option><option value="month">{t("本月")}</option></select></div><div className="filter-field"><label htmlFor="provider-filter">{t("来源")}</label><select id="provider-filter" value={providerId} disabled={providersQuery.isPending} onChange={event => changeProvider(event.target.value)}><option value="">{t("全部来源")}</option>{providers.map(provider => <option value={provider.providerId} key={provider.providerId}>{provider.displayName}</option>)}</select></div><div className="filter-field"><label htmlFor="model-filter">{t("模型")}</label><select id="model-filter" value={modelSupported ? modelId : ''} disabled={!modelSupported} title={!modelSupported ? t("所选来源不支持模型筛选") : undefined} onChange={event => { setModelId(event.target.value); resetPagination(); }}><option value="">{t("全部模型")}</option>{models.map(model => <option value={model.id} key={model.id}>{model.id}</option>)}</select></div><div className="filter-field"><label htmlFor="bucket-filter">{t("趋势粒度")}</label><select id="bucket-filter" value={bucket} disabled={page === 'sessions' || !overviewSupported} onChange={event => setBucket(event.target.value as Bucket)}><option value="day">{t("每日")}</option><option value="week">{t("每周")}</option><option value="month">{t("每月")}</option></select></div><div className="filter-summary"><Icon name="clock" /><span>{displayRange(range)} <span className="muted">· {timezone}</span></span>{!modelSupported && <span className="filter-hint">{t("所选来源不支持模型筛选")}</span>}</div></div></FilterPopover></div>
        {scan.pending || scan.job ? <Notice title={scan.job ? t("扫描任务进行中") : t("正在启动扫描")} action={<div className="button-group">{scan.job && !scan.pending && <button className="button button-small" onClick={scan.resume}>{t("继续等待")}</button>}<button className="button button-small" disabled={!scan.job || scan.cancelling} onClick={scan.cancel}>{scan.cancelling ? t("正在取消…") : t("取消扫描")}</button></div>}>{t("扫描期间可继续浏览已有数据。")}{scan.job && <span className="small muted">{t('任务 {id}', { id: scan.job.jobId })}</span>}</Notice> : scan.outcome && <div className={`scan-result-inline ${scan.outcome.state === 'failed' ? 'warning-text' : ''}`} role="status"><StatusBadge state={scan.outcome.state} />{scan.outcome.state === 'failed' ? t(" 上次成功数据已保留。") : scan.outcome.state === 'cancelled' ? t(" 已取消，已有快照未清空。") : demo ? t(" 示例数据已刷新。") : t(" 已更新用量快照。")}</div>}
        {scan.error && <ErrorNotice error={scan.error} />}{setupError !== null && <ErrorNotice error={normalizeError(setupError)} />}{!demo && providers.length > 0 && providers.every(provider => !provider.lastSuccessAt) && page !== 'settings' && <Notice title={t("先启用来源并扫描")}><p>{t("当前还没有采集记录。到数据来源页面点击 Codex 或 Antigravity 的“启用并扫描”；默认目录找不到时，可在设置中选择本机用量目录。")}</p>{page !== 'providers' && <button className="button button-small" onClick={() => { window.location.hash = 'providers'; }}>{t("前往数据来源")}</button>}</Notice>}
        {exporting.isPending && <Notice>{t("正在准备导出…")}</Notice>}
        {exporting.isError && (normalizeError(exporting.error).code === 'CANCELLED' ? <Notice>{t("已取消保存。")}</Notice> : <ErrorNotice error={normalizeError(exporting.error)} />)}
        {exporting.data && <Notice tone="success">{demo ? t("演示导出请求完成，未保存文件。") : t('已保存 {filename}。', { filename: exporting.data.suggestedFilename })}</Notice>}
        {providersQuery.isError && <ErrorNotice error={normalizeError(providersQuery.error)} retry={() => { void providersQuery.refetch(); }} />}
        {providersQuery.isPending ? <LoadingState label={t("正在加载来源信息…")} /> : <>
          {page === 'overview' && <>{overview.isError && <ErrorNotice error={normalizeError(overview.error)} retry={() => { void overview.refetch(); }} />}{overview.data ? <Overview result={overview.data} providers={providers} timezone={timezone} /> : overview.isLoading || timezoneLoading ? <LoadingState /> : (!overviewSupported || !api.data.capabilities.includes('overview')) && <Panel title={t("用量概览")}><EmptyState title={t("此服务暂不支持概览")} /></Panel>}</>}
          {page === 'providers' && <Providers providers={providers} timezone={timezone} canScan={api.data.capabilities.includes('scan-polling') && timezoneReady} busy={enabling || settings.save.isPending || scan.pending || !!scan.job} onScan={scan.start} onEnableAndScan={settings.canWrite && api.data.capabilities.includes('settings-write') && settings.query.data ? providerId => { void enableAndScan(providerId); } : undefined} onConfigure={() => { window.location.hash = 'settings'; }} />}
          {page === 'sessions' && <>{sessions.isError && <ErrorNotice error={normalizeError(sessions.error)} retry={() => { void sessions.refetch(); }} />}{(sessions.isLoading || timezoneLoading) && <LoadingState label={t("正在加载会话数据…")} />}{!providerId && sessionProviders.length < selected.length && <Notice>{t("仅列出支持会话报表的来源。其余来源的每日用量仍可在概览查看。")}</Notice>}<Sessions result={sessions.data} providers={providers} timezone={timezone} activeOnly={activeOnly} onActiveOnly={value => { setActiveOnly(value); resetPagination(); }} offset={offset} supported={sessionProviders.length > 0 && api.data.capabilities.includes('sessions')} onNext={() => { if (sessions.data?.nextOffset !== null && sessions.data?.nextOffset !== undefined) { setPreviousOffsets([...previousOffsets, offset]); setOffset(sessions.data.nextOffset); } }} onPrevious={() => { setOffset(previousOffsets.at(-1) ?? 0); setPreviousOffsets(previousOffsets.slice(0, -1)); }} /></>}
          {page === 'settings' && <Settings key={settings.query.data?.revision ?? 'unavailable'} theme={theme} setTheme={setTheme} timezone={timezone} setTimezone={value => { setLegacyTimezone(value); resetPagination(); }} demo={demo} providers={providers} api={api.data} modelFiltered={query.modelIds.length > 0} exporting={exporting.isPending} onExport={doExport}
            settingsLoading={api.data.capabilities.includes('settings-read') && settings.canRead && settings.query.isPending}
            settingsError={settings.query.error ? normalizeError(settings.query.error) : undefined}
            automatic={api.data.capabilities.includes('auto-full-scan') ? { status: automatic.query.data, pending: automatic.save.isPending, cancelling: automatic.cancel.isPending,
              error: automatic.query.error ?? automatic.save.error ?? automatic.cancel.error, save: request => automatic.save.mutate(request), cancel: jobId => automatic.cancel.mutate(jobId), reload: () => { automatic.save.reset(); void automatic.query.refetch(); } } : undefined}
            timezoneControls={followsHost ? { status: zone.query.data, pending: zone.save.isPending, error: zone.save.error ?? (zone.query.data ? zone.query.error : null), save: request => zone.save.mutate(request), reload: () => { zone.save.reset(); void zone.query.refetch(); } } : undefined}
            controls={settings.query.data && settings.canWrite && api.data.capabilities.includes('settings-write') ? { value: settings.query.data, pending: settings.save.isPending, choosing: settings.choose.isPending, busy: scan.pending || !!scan.job || backgroundScanning || automaticJobs.length > 0 || enabling, saved: settings.save.isSuccess,
              error: settings.save.error ? normalizeError(settings.save.error) : settings.choose.error ? normalizeError(settings.choose.error) : undefined,
              save: request => settings.save.mutate(request), choose: settings.canChoose && api.data.capabilities.includes('source-directory-selection') ? async providerId => (await settings.choose.mutateAsync(providerId)).directory : undefined,
              reload: () => { settings.save.reset(); settings.choose.reset(); void settings.query.refetch(); },
            } : undefined} />}
        </>}
      </>}
      <footer className="app-footer"><span>Agent Usage Dashboard <span className="footer-dot">·</span> {t("本地优先")}</span><span>{demo ? t("所有用量与价格均为合成示例") : t("API 等价估算不代表实际账单")}</span></footer>
    </main></div>
  </div>;
}
