import type { TimezoneProviderState, TimezoneRebuildState, TimezoneStatus } from '../api/generated/usage';
import { formatTime } from '../components/format';
import { Notice } from '../components/ui';
import { useI18n } from '../i18n/I18nContext';
import { localizeError, type MessageKey } from '../i18n/messages';

const titles: Record<Exclude<TimezoneRebuildState, 'idle'>, MessageKey> = { pending: '等待按新统计时区重建', rebuilding: '正在按新统计时区重建', backoff: '统计时区重建未完成，等待重试' };
const providerStates: Record<TimezoneProviderState, MessageKey> = { pending: '等待重建', rebuilding: '正在重建', succeeded: '已完成重建', failed: '重建失败' };
/** Rebuild progress, pending follow-system switches and detection failures reported by the host (API 1.3). */
export function TimezoneNotice({ status, names, cancelling = false, onCancel }: { status?: TimezoneStatus; names: Record<string, string>; cancelling?: boolean; onCancel?: (jobId: string) => void }) {
  const { t, language } = useI18n();
  if (!status) return null;
  const zone = status.effectiveTimezone;
  return <>
    {status.rebuild !== 'idle' && <Notice tone={status.rebuild === 'backoff' ? 'warning' : 'info'} title={t(titles[status.rebuild])}><p>{t('统计时区为 {timezone}，正在按该时区从来源日志重新统计已启用来源。完成前该时区的统计可能不完整；旧时区的结果不会作为新时区数据显示，缺失的数值保持不可用。', { timezone: zone })}</p>
      {status.rebuild === 'backoff' && <p>{t('已有历史已保留，将于 {time} 自动重试。', { time: formatTime(status.nextRetryAt, zone, language) })}</p>}
      {status.providers.length > 0 && <ul className="timezone-providers" aria-label={t('各来源重建进度')}>{status.providers.map(provider => <li key={provider.providerId}><span>{`${names[provider.providerId] ?? provider.providerId} · ${t(providerStates[provider.state])}`}</span>{provider.error && <span className="warning-text"> · {localizeError(provider.error, language)}</span>}{provider.state === 'rebuilding' && provider.jobId && onCancel && <button className="button button-small" disabled={cancelling} onClick={() => onCancel(provider.jobId!)}>{t(cancelling ? '正在取消…' : '取消重建')}</button>}</li>)}</ul>}
    </Notice>}
    {status.pendingTimezone && <Notice title={t('系统时区已变化')}>{t('系统时区已变为 {timezone}，将在当前扫描结束后切换统计时区。', { timezone: status.pendingTimezone })}</Notice>}
    {status.detectionError && <Notice tone="warning" title={t('无法检测系统时区')}>{t('继续使用当前统计时区 {timezone}，并会自动重试检测。', { timezone: zone })}</Notice>}
  </>;
}
