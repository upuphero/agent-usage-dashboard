import { useState } from 'react';
import type { AutoCollectionState, AutoCollectionStatus, UpdateAutoCollectionRequest } from '../api/generated/usage';
import { normalizeError } from '../api/protocol';
import { ErrorNotice, Notice, Panel } from '../components/ui';
import { formatTime } from '../components/format';
import { useI18n } from '../i18n/I18nContext';
import type { MessageKey } from '../i18n/messages';

export interface AutomaticControls {
  status?: AutoCollectionStatus; pending: boolean; cancelling: boolean; error?: unknown;
  save: (request: UpdateAutoCollectionRequest) => void; cancel: (jobId: string) => void; reload: () => void;
}
const states: Record<AutoCollectionState, MessageKey> = {
  disabled: '已关闭', idle: '等待下次检查', checking: '正在检查来源', scanning: '自动完整扫描进行中', waiting: '等待合并或扫描间隔', backoff: '失败后等待重试',
};
function Editor({ controls }: { controls: AutomaticControls & { status: AutoCollectionStatus } }) {
  const { t } = useI18n();
  const [config, setConfig] = useState(controls.status.config);
  const dirty = config.enabled !== controls.status.config.enabled || config.intervalMinutes !== controls.status.config.intervalMinutes;
  return <><div className="setting-row"><div><h3>{t('自动采集')}</h3><p className="small muted">{t('定时检查已启用来源，有变化时完整扫描。软件关闭后停止。')}</p></div><label className="checkbox-label"><input type="checkbox" aria-label={t('自动采集')} checked={config.enabled} disabled={controls.pending} onChange={event => setConfig({ ...config, enabled: event.target.checked })} />{t(config.enabled ? '已启用' : '已关闭')}</label></div>
    <div className="setting-row"><label htmlFor="auto-interval">{t('检查间隔')}</label><select id="auto-interval" value={config.intervalMinutes} disabled={controls.pending} onChange={event => setConfig({ ...config, intervalMinutes: Number(event.target.value) })}>{[1, 5, 15].map(value => <option key={value} value={value}>{value === 1 ? t('1 分钟') : t('{minutes} 分钟', { minutes: value })}</option>)}</select></div>
    <p className="small muted">{t('关闭自动采集会清除待执行请求并取消尚未提交的自动任务；已提交结果保留，手动任务继续。')}</p>
    <div className="button-group"><button className="button" disabled={controls.pending || !dirty} onClick={() => controls.save({ expectedRevision: controls.status.revision, config })}>{t(controls.pending ? '正在保存…' : '保存自动采集设置')}</button><button className="button" disabled={controls.pending} onClick={controls.reload}>{t('重新读取')}</button></div></>;
}
export function AutomaticCollection({ controls, names }: { controls?: AutomaticControls; names: Record<string, string> }) {
  const { t, language } = useI18n(); const status = controls?.status;
  return <Panel title={t('自动采集')} eyebrow={t('完整扫描')}>
    {!controls ? <Notice>{t('当前服务不支持自动完整扫描。')}</Notice> : !status ? <Notice>{t('正在读取自动采集状态…')}</Notice> : <>
      <Editor key={status.revision} controls={{ ...controls, status }} />
      <p role="status">{t('自动采集状态')}：{t(status.config.enabled ? '已启用' : '已关闭')} · {status.timezone}</p>
      {status.config.enabled && status.providers.length === 0 && <Notice>{t('尚无已启用来源，启用来源后开始检查。')}</Notice>}
      {status.providers.map(provider => <div className="setting-row" key={provider.providerId}><div><h3>{names[provider.providerId] ?? provider.providerId}</h3><p role="status">{t(states[provider.state])}</p><p className="small muted">{t('最后成功扫描')}：{formatTime(provider.lastSuccessAt, status.timezone, language)} · {t('下次检查')}：{formatTime(provider.nextCheckAt, status.timezone, language)}</p>{!provider.watching && <p className="small muted">{t('文件监听不可用，使用间隔检查。')}</p>}{provider.error && <ErrorNotice error={provider.error} />}</div>{provider.jobId && <button className="button button-small" disabled={controls.cancelling} onClick={() => controls.cancel(provider.jobId!)}>{t(controls.cancelling ? '正在取消…' : '取消自动扫描')}</button>}</div>)}
    </>}
    {controls?.error != null && <ErrorNotice error={normalizeError(controls.error)} />}
  </Panel>;
}
