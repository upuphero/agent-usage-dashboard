import type { Accuracy, Metric } from '../api/generated/usage';

export const accuracyLabels: Record<Accuracy, string> = {
  exact: '来源报告', derived: '推导值', estimated: '估算', unavailable: '不可用',
};
export function formatTokens(value: string | null): string {
  if (value === null) return '不可用';
  try { return new Intl.NumberFormat('en-US').format(BigInt(value)); } catch { return '不可用'; }
}

/** Chinese reading aid only; the exact decimal string remains the primary value. */
export function formatChineseMagnitude(value: string | null, locale = 'zh-CN'): string | null {
  if (!locale.toLowerCase().startsWith('zh') || value === null || !/^\d+$/.test(value)) return null;
  const number = BigInt(value);
  if (number < 10_000n) return null;
  const units = [[10_000_000_000_000_000n, '京'], [1_000_000_000_000n, '兆'], [100_000_000n, '亿'], [10_000n, '万']] as const;
  const index = units.findIndex(([scale]) => number >= scale);
  const [scale, name] = units[index];
  let text = `${number / scale}${name}`;
  let remainder = number % scale;
  const next = units[index + 1];
  if (next) {
    if (remainder / next[0] > 0n) text += `${remainder / next[0]}${next[1]}`;
    remainder %= next[0];
  } else if (remainder > 0n) { text += remainder.toString(); remainder = 0n; }
  return `${remainder > 0n ? '约' : ''}${text}`;
}

/** Compact axis text. All rounding is done with integers, never unsafe token Numbers. */
export function formatChartCount(value: string): string {
  const number = BigInt(value);
  for (const [scale, unit] of [[1_000_000_000_000n, '万亿'], [100_000_000n, '亿'], [10_000n, '万']] as const) {
    if (number < scale) continue;
    const tenths = (number * 10n + scale / 2n) / scale;
    return `${tenths / 10n}${tenths % 10n ? `.${tenths % 10n}` : ''}${unit}`;
  }
  return number.toString();
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

/** Exact, nonnegative decimal ordering for token/cost presentation. Unknown stays last. */
export function compareDecimalDescending(a: string | null, b: string | null): number {
  if (a === null) return b === null ? 0 : 1;
  if (b === null) return -1;
  const [aWhole, aFraction = ''] = a.split('.');
  const [bWhole, bFraction = ''] = b.split('.');
  const aw = BigInt(aWhole); const bw = BigInt(bWhole);
  if (aw !== bw) return aw > bw ? -1 : 1;
  const length = Math.max(aFraction.length, bFraction.length);
  const af = aFraction.padEnd(length, '0'); const bf = bFraction.padEnd(length, '0');
  return af === bf ? 0 : af > bf ? -1 : 1;
}
export function decimalBarPercent(value: string | null, maximum: string | null): number {
  if (value === null || maximum === null) return 0;
  const [whole, fraction = ''] = value.split('.'); const [maxWhole, maxFraction = ''] = maximum.split('.');
  const length = Math.max(fraction.length, maxFraction.length);
  const count = BigInt(whole + fraction.padEnd(length, '0')); const max = BigInt(maxWhole + maxFraction.padEnd(length, '0'));
  return max === 0n ? 0 : Math.min(100, Number(count * 1000n / max) / 10);
}
