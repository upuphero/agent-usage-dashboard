import { useState } from 'react';
import type { TimezoneMode, TimezoneStatus, UpdateTimezoneRequest } from '../api/generated/usage';
import { normalizeError } from '../api/protocol';
import { ErrorNotice } from '../components/ui';
import { useI18n } from '../i18n/I18nContext';

/** API 1.3 controls; absent for older services, which keep the legacy timezone row. */
export interface TimezoneControls { status?: TimezoneStatus; pending: boolean; error?: unknown; save: (request: UpdateTimezoneRequest) => void; reload: () => void }
const FALLBACK_ZONES = ['America/Phoenix', 'UTC', 'Asia/Shanghai', 'America/Los_Angeles'];
/** Every IANA zone the runtime knows (short list on older WebViews) plus the host's current zones. */
export function timezoneOptions(...current: Array<string | null>): string[] {
  let zones = FALLBACK_ZONES;
  try { if (typeof Intl.supportedValuesOf === 'function') zones = Intl.supportedValuesOf('timeZone'); } catch { /* keep the short list */ }
  return [...new Set([...zones, 'UTC', ...current.filter((zone): zone is string => !!zone)])].sort();
}
/** Revision-guarded write; follow-system never sends a zone. */
export const timezoneRequest = (status: TimezoneStatus, mode: TimezoneMode, zone: string): UpdateTimezoneRequest => ({ expectedRevision: status.revision, mode, timezone: mode === 'fixed' ? zone : null });
function Editor({ controls, demo }: { controls: TimezoneControls & { status: TimezoneStatus }; demo: boolean }) {
  const { t } = useI18n(); const { status } = controls;
  const [mode, setMode] = useState<TimezoneMode>(status.mode);
  const [zone, setZone] = useState(status.effectiveTimezone);
  const dirty = mode !== status.mode || (mode === 'fixed' && zone !== status.effectiveTimezone);
  return <><fieldset className="timezone-mode" disabled={controls.pending}><legend>{t('统计时区模式')}</legend>{(['follow-system', 'fixed'] as const).map(value => <label className="checkbox-label" key={value}><input type="radio" name="timezone-mode" value={value} checked={mode === value} onChange={() => setMode(value)} />{t(value === 'fixed' ? '固定时区' : '跟随系统时区（推荐）')}</label>)}</fieldset>
    <div className="setting-row"><div><h3>{t('固定统计时区')}</h3><p className="small muted">{t('仅在固定时区模式下使用。')}</p>{demo && <p className="small muted">{t('演示数据仅提供 America/Phoenix。')}</p>}</div><select aria-label={t('固定统计时区')} value={zone} disabled={mode !== 'fixed' || controls.pending} onChange={event => setZone(event.target.value)}>{timezoneOptions(status.effectiveTimezone, status.systemTimezone, zone).map(value => <option key={value} value={value}>{value}</option>)}</select></div>
    <p className="small muted">{t('更改统计时区会按新时区从来源日志重新统计已启用来源的用量；已汇总的每日总量不会被直接改标为新时区。')}</p>
    <div className="button-group"><button className="button" disabled={controls.pending || !dirty} onClick={() => controls.save(timezoneRequest(status, mode, zone))}>{t(controls.pending ? '正在保存…' : '保存统计时区')}</button><button className="button" disabled={controls.pending} onClick={controls.reload}>{t('重新读取')}</button></div></>;
}
export function TimezoneSettings({ controls, demo }: { controls: TimezoneControls; demo: boolean }) {
  const { t } = useI18n(); const status = controls.status;
  return <div className="timezone-settings"><div className="setting-row"><div><h3>{t('统计时区')}</h3>{status ? <><p role="status">{t('当前统计时区：{timezone}', { timezone: status.effectiveTimezone })} · {t(status.mode === 'fixed' ? '固定时区' : '跟随系统时区')}</p><p role="status" className="small muted">{status.systemTimezone ? t('系统时区：{timezone}', { timezone: status.systemTimezone }) : t('系统时区：无法检测')}{status.detectionError && ` · ${t('最近一次检测失败')}`}</p></> : controls.error == null && <p className="small muted">{t('正在读取统计时区…')}</p>}</div></div>
    {status && <Editor key={status.revision} controls={{ ...controls, status }} demo={demo} />}
    {controls.error != null && <ErrorNotice error={normalizeError(controls.error)} retry={controls.reload} />}</div>;
}
