import { useState } from 'react';
import type { ApiInfo, ApiError, ExportFormat, ProviderSummary, SettingsResult, UpdateSettingsRequest, SourceDirectory } from '../api/generated/usage';
import { Icon } from '../components/Icon';
import { ErrorNotice, Notice, Panel } from '../components/ui';
import { useI18n } from '../i18n/I18nContext';
import { localizeSourceText } from '../i18n/messages';
import { AutomaticCollection, type AutomaticControls } from './AutomaticCollection';
import { TimezoneSettings, type TimezoneControls } from './TimezoneSettings';

export type Theme = 'dark' | 'light' | 'system';
export interface SettingsControls { value: SettingsResult; pending: boolean; choosing: boolean; busy: boolean; saved?: boolean; error?: ApiError; save: (request: UpdateSettingsRequest) => void; choose?: (providerId: string) => Promise<SourceDirectory | null>; reload: () => void }
export function Settings({ theme, setTheme, timezone, setTimezone, demo, providers, api, modelFiltered, exporting, onExport, controls, automatic, timezoneControls, settingsLoading = false, settingsError }: { theme: Theme; setTheme: (theme: Theme) => void; timezone: string; setTimezone: (timezone: string) => void; demo: boolean; providers: ProviderSummary[]; api: ApiInfo; modelFiltered: boolean; exporting: boolean; onExport: (format: ExportFormat) => void; controls?: SettingsControls; automatic?: AutomaticControls; timezoneControls?: TimezoneControls; settingsLoading?: boolean; settingsError?: ApiError }) {
  const { t, language } = useI18n();
  const initial = controls ? { timezone: controls.value.timezone, providers: controls.value.providers.map(provider => ({ providerId: provider.providerId, enabled: provider.enabled, directoryRef: provider.directory?.directoryRef ?? null })) } : null;
  const [draft, setDraft] = useState(initial);
  const [labels, setLabels] = useState<Record<string, string>>({});
  const busy = !!controls && (controls.pending || controls.choosing || controls.busy);
  const dirty = !!draft && JSON.stringify(draft) !== JSON.stringify(initial);
  const edit = (providerId: string, values: Partial<UpdateSettingsRequest['providers'][number]>) => setDraft(value => value && ({ ...value, providers: value.providers.map(provider => provider.providerId === providerId ? { ...provider, ...values } : provider) }));
  const choose = async (providerId: string) => { const directory = await controls?.choose?.(providerId); if (directory) { edit(providerId, { directoryRef: directory.directoryRef }); setLabels(value => ({ ...value, [providerId]: directory.label })); } };
  const selectedTimezone = draft?.timezone ?? timezone;
  const zones = ['America/Phoenix', 'UTC', 'Asia/Shanghai', 'America/Los_Angeles'];
  if (!zones.includes(selectedTimezone)) zones.push(selectedTimezone);
  return <div className="page-stack"><Panel title={t("显示与统计偏好")} eyebrow={t('偏好')}>
    <div className="setting-row"><div><h3>{t("外观")}</h3><p className="small muted">{t("保存在当前浏览器或桌面 WebView 中。")}</p></div><select aria-label={t("主题")} value={theme} onChange={event => setTheme(event.target.value as Theme)}><option value="dark">{t("深色")}</option><option value="light">{t("浅色")}</option><option value="system">{t("跟随系统")}</option></select></div>
    {timezoneControls ? <TimezoneSettings controls={timezoneControls} demo={demo} /> : <div className="setting-row"><div><h3>{t("统计查询时区")}</h3><p className="small muted">{t("改变时区后需要重新扫描，已有每日总量不能直接重分桶。")}{demo ? t("演示快照的时区固定。") : controls ? t("保存后用于后续查询。") : t("当前服务仅支持临时查询时区。")}</p></div><select aria-label={t("统计时区")} value={selectedTimezone} disabled={demo || busy} onChange={event => draft ? setDraft({ ...draft, timezone: event.target.value }) : setTimezone(event.target.value)}>{zones.map(zone => <option key={zone}>{zone}</option>)}</select></div>}
  </Panel><Panel title={t("来源开关")} eyebrow={t('来源设置')}>
    {settingsLoading && <Notice>{t("正在读取保存的配置…")}</Notice>}
    {(settingsError || controls?.error) && <ErrorNotice error={(settingsError ?? controls?.error)!} />}
    <p className="small muted">{controls ? localizeSourceText(controls.value.collectionNotice, language) : t('当前仅显示来源配置。来源开关与目录配置尚未开放修改。')}</p>
    {controls?.value.directoryChangePolicy === 'preserve-dataset' && <p className="small muted">{t("目录更换适用于同一份日志的迁移；切换独立数据集暂未开放。关闭来源会保留已有历史。")}</p>}
    <div className="settings-providers">{providers.length ? providers.map(provider => {
      const current = draft?.providers.find(item => item.providerId === provider.providerId);
      const original = controls?.value.providers.find(item => item.providerId === provider.providerId);
      const label = current ? current.directoryRef === null ? t("默认日志目录") : labels[provider.providerId] ?? original?.directory?.label ?? t("已选择自定义目录") : provider.pathHint ?? t("路径不可用");
      return <div className="setting-row" key={provider.providerId}><div><h3>{provider.displayName}</h3><p className="small muted">{localizeSourceText(label, language)}</p><div className="button-group"><button className="button button-small" disabled={!current || !controls?.choose || busy} onClick={() => { void choose(provider.providerId).catch(() => undefined); }}>{controls?.choosing ? t("正在选择…") : t("选择目录")}</button><button className="button button-small" disabled={!current || current.directoryRef === null || busy} onClick={() => edit(provider.providerId, { directoryRef: null })}>{t("使用默认目录")}</button></div></div><label className={`checkbox-label ${!current ? 'disabled' : ''}`}><input type="checkbox" aria-label={t('启用 {provider}', { provider: provider.displayName })} checked={current?.enabled ?? provider.enabled} disabled={!current || busy} onChange={event => edit(provider.providerId, { enabled: event.target.checked })} />{(current?.enabled ?? provider.enabled) ? t("已启用") : t("已关闭")}</label></div>;
    }) : <p className="muted">{t("尚无来源配置。")}</p>}</div>
    {controls && <div className="button-group"><button className="button" disabled={busy || !dirty} onClick={() => { if (draft) controls.save({ expectedRevision: controls.value.revision, ...draft, timezone: timezoneControls ? controls.value.timezone : draft.timezone }); }}>{controls.pending ? t("正在保存…") : demo ? t("保存演示设置") : t("保存统计设置")}</button><button className="button" disabled={busy} onClick={controls.reload}>{t("重新读取")}</button></div>}
    {controls?.busy && <Notice>{t("扫描进行中，结束后可修改设置。")}</Notice>}
    {controls?.saved && !dirty && <Notice tone="success">{demo ? t("演示设置已保存，未修改磁盘文件。") : timezoneControls ? t("来源设置已保存。") : t("统计设置已保存。若更改了时区，请重新扫描来源。")}</Notice>}
    {demo && controls && <Notice>{t("演示设置仅保留在当前页面会话，刷新后恢复；目录选择不访问系统文件。")}</Notice>}
  </Panel>
  <AutomaticCollection controls={automatic} names={Object.fromEntries(providers.map(provider => [provider.providerId, provider.displayName]))} />
  <Panel title={t("导出")} eyebrow={t("导出报表")}><div className="setting-row"><div><h3>{t("JSON 完整历史归档")}</h3><p className="small muted">{t("包含所选来源与时区的完整历史及数据集身份；不受日期范围限制。")}</p>{modelFiltered && <p className="small warning-text">{t("请先清除模型筛选，再导出完整归档。")}</p>}</div><button className="button" disabled={exporting || modelFiltered || !api.capabilities.includes('export-json-full-history')} onClick={() => onExport('json')}><Icon name="download" />{t("导出 JSON")}</button></div><div className="setting-row"><div><h3>{t("CSV 用量报表")}</h3><p className="small muted">{t("使用当前日期、来源与模型筛选，用于阅读分析。")}</p></div><button className="button" disabled={exporting || !api.capabilities.includes('export-csv')} onClick={() => onExport('csv')}><Icon name="download" />{t("导出 CSV")}</button></div>{demo && <Notice>{t("演示导出仅模拟请求与结果，不会保存文件。")}</Notice>}</Panel>
  <Panel title={t("本地数据与隐私")} eyebrow={t("本地优先")}><div className="privacy-heading"><Icon name="shield" /><div><h3>{t("用量留在本机")}</h3><p className="small muted">{t("本地统计默认无遥测；仅提取用量元数据，不持久化或上传聊天正文。")}</p></div></div><div className="setting-row"><div><h3>{t("数据目录与维护")}</h3><p className="small muted">{t("目录选择、备份、清除与开机启动暂未开放。")}</p></div><button className="button" disabled>{t("管理数据")}</button></div><div className="panel-footer"><span>{t('应用 {appVersion} · 接口 {apiVersion}', { appVersion: api.appVersion, apiVersion: api.apiVersion })}</span><span>Windows x64 / macOS Apple Silicon ARM64</span></div></Panel></div>;
}
