import type { DateRange } from '../api/generated/usage';

export type DatePreset = 'today' | 'week' | 'month' | 'last30';
const iso = (date: Date) => date.toISOString().slice(0, 10);
export function systemTimezone(): string {
  return new Intl.DateTimeFormat().resolvedOptions().timeZone;
}
export function todayInTimezone(timezone: string): string {
  const parts = new Intl.DateTimeFormat('en-US', { timeZone: timezone, year: 'numeric', month: '2-digit', day: '2-digit' }).formatToParts(new Date());
  const part = (type: string) => parts.find(value => value.type === type)!.value;
  return `${part('year')}-${part('month')}-${part('day')}`;
}
export function rangeForPreset(today: string, preset: DatePreset): DateRange {
  const date = new Date(`${today}T00:00:00Z`);
  if (preset === 'month') return { start: `${today.slice(0, 7)}-01`, end: iso(new Date(Date.UTC(date.getUTCFullYear(), date.getUTCMonth() + 1, 1))) };
  const tomorrow = new Date(date); tomorrow.setUTCDate(date.getUTCDate() + 1);
  if (preset === 'week') date.setUTCDate(date.getUTCDate() - ((date.getUTCDay() + 6) % 7));
  if (preset === 'last30') date.setUTCDate(date.getUTCDate() - 29);
  return { start: iso(date), end: iso(tomorrow) };
}
export function displayRange(range: DateRange): string {
  const last = new Date(`${range.end}T00:00:00Z`); last.setUTCDate(last.getUTCDate() - 1);
  return `${range.start} — ${iso(last)}`;
}
