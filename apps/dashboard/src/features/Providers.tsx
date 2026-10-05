import type { ProviderSummary } from '../api/generated/usage';
import { Icon } from '../components/Icon';
import { formatTime } from '../components/format';
import { CoverageDetails, EmptyState, Panel, StatusBadge } from '../components/ui';
import { useI18n } from '../i18n/I18nContext';
import { localizeError, localizeSourceText } from '../i18n/messages';

export function Providers({ providers, timezone, canScan, busy, onScan, onEnableAndScan, onConfigure }: { providers: ProviderSummary[]; timezone: string; canScan: boolean; busy: boolean; onScan: (id: string) => void; onEnableAndScan?: (id: string) => void; onConfigure?: () => void }) {
  const { t, language } = useI18n();
  if (!providers.length) return <Panel title={t("本机数据来源")}><EmptyState title={t("尚未连接数据来源")} description={t("来源检测和采集服务就绪后会在此显示。当前没有已连接的来源。")} /></Panel>;
  return <div className="page-stack"><div className="provider-grid">{providers.map((provider, index) => <article className="panel provider-card" key={provider.providerId}>
    <div className="provider-header"><div className={`provider-mark color-${index % 3}`}>{provider.displayName.slice(0, 1)}</div><div><h2>{provider.displayName}</h2><p className="small muted">{provider.productId}</p></div>{provider.enabled ? <StatusBadge state={provider.state} /> : <span className="pill">{t("未启用")}</span>}</div>
    <div className="provider-path"><Icon name="providers" /><span>{provider.pathHint ? localizeSourceText(provider.pathHint, language) : t('默认目录未找到；可在设置中选择用量目录')}</span></div>
    {!provider.lastSuccessAt && <p className="small muted">{provider.enabled ? t("首次扫描后才会显示用量。找不到默认目录时，请选择这个工具的本机日志目录。") : t("来源尚未启用，因此还没有采集数据。启用并扫描后即可查看用量。")}</p>}
    <dl className="provider-details"><div><dt>{t("最后成功扫描")}</dt><dd>{formatTime(provider.lastSuccessAt, timezone, language)}</dd></div><div><dt>{t("采集方式")}</dt><dd>{provider.capabilities.supportsIncrementalCollection ? t("增量采集") : t("完整扫描")}</dd></div><div><dt>{t("可用报表")}</dt><dd>{provider.capabilities.reportKinds.map(kind => kind === 'daily' ? t("每日") : t("会话")).join(language === 'zh' ? '、' : ', ') || t("暂无")}</dd></div></dl>
    <div className="capability-tags"><span className="small muted">{t("支持维度")}</span>{provider.capabilities.supportedDimensions.map(dimension => <span className="pill" key={dimension}>{({ day: t("日期"), model: t("模型"), session: t("会话"), project: t("项目") } as Record<string, string>)[dimension] ?? dimension}</span>)}</div>
    <CoverageDetails coverage={provider.coverage} />
    {provider.lastScan && <div className="provider-last-scan"><StatusBadge state={provider.lastScan.state} />{provider.lastScan.error && <p className="small warning-text">{provider.lastScan.error.code} · {localizeError(provider.lastScan.error, language)}</p>}</div>}
    <div className="provider-actions"><span className="small muted">{t("仅统计本机保留的用量")}{!provider.capabilities.supportsQuota ? t(" · 额度不可用") : ''}</span><button className="button button-small" disabled={!canScan || (!provider.enabled && !onEnableAndScan) || busy || provider.state === 'scanning'} onClick={() => provider.enabled ? onScan(provider.providerId) : onEnableAndScan?.(provider.providerId)}><Icon name="refresh" />{provider.state === 'scanning' ? t("扫描中") : provider.enabled ? t("扫描来源") : t("启用并扫描")}</button></div>
    {onConfigure && <button className="button button-small" disabled={busy} onClick={onConfigure}>{t("选择目录 / 修改设置")}</button>}
  </article>)}</div><Panel title={t("统计范围")}><p className="muted">{t("每个来源独立声明支持的报表、维度和字段。不可用的筛选会禁用；来源不可读、格式不兼容或扫描失败时，保留上次成功的数据。")}</p><p className="panel-footnote">{t("本机日志中的用量不代表账号所有云端活动。Codex、Claude Code 与网页产品分别统计。")}</p></Panel></div>;
}
