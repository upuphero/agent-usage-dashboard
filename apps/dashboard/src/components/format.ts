import type { Accuracy, Metric } from '../api/generated/usage';

export const accuracyLabels: Record<Accuracy, string> = {
  exact: '来源报告', derived: '推导值', estimated: '估算', unavailable: '不可用',
};
export function formatTokens(value: string | null): string {
  if (value === null) return '不可用';
  try { return new Intl.NumberFormat('en-US').format(BigInt(value)); } catch { return '不可用'; }
}
export function formatUsd(value: string | null): string {
  if (value === null || !/^\d+(?:\.\d+)?$/.test(value)) return '不可用';
  const [whole, fraction = ''] = value.split('.');
  if (BigInt(whole) === 0n && /[1-9]/.test(fraction) && (fraction.padEnd(2, '0').slice(0, 2) === '00')) return '< $0.01';
  const cents = BigInt(whole) * 100n + BigInt(fraction.padEnd(2, '0').slice(0, 2)) + (Number(fraction[2] ?? '0') >= 5 ? 1n : 0n);
  return `$${new Intl.NumberFormat('en-US').format(cents / 100n)}.${(cents % 100n).toString().padStart(2, '0')}`;
}
export function formatTime(value: string | null, timezone: string): string {
  if (!value) return '尚无记录';
  const date = new Date(value);
  if (Number.isNaN(date.getTime())) return '时间不可用';
  return new Intl.DateTimeFormat('zh-CN', { timeZone: timezone, month: '2-digit', day: '2-digit', hour: '2-digit', minute: '2-digit', hour12: false }).format(date);
}
export function qualityDescription(metric: Metric<string>): string {
  if (metric.value === null) return '来源未提供该字段，不能视为零。';
  return `${accuracyLabels[metric.accuracy]} · ${metric.knownRows} 条已知记录${metric.missingRows ? ` · ${metric.missingRows} 条缺失，仅显示已知部分` : ''}`;
}

/** Scale chart geometry only. Never convert token strings to unsafe JS integers. */
export function barPercent(value: string | null, maximum: string | null): number {
  if (value === null || maximum === null || BigInt(maximum) === 0n) return 0;
  return Math.min(100, Number(BigInt(value) * 1000n / BigInt(maximum)) / 10);
}
export function largestMetric(values: Array<string | null>): string | null {
  return values.reduce<string | null>((max, value) => value !== null && (max === null || BigInt(value) > BigInt(max)) ? value : max, null);
}
