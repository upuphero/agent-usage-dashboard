import { afterEach, describe, expect, it, vi } from 'vitest';
import { displayRange, rangeForPreset, systemTimezone, todayInTimezone } from './dates';

afterEach(() => { vi.useRealTimers(); vi.restoreAllMocks(); });

describe('statistics follow the selected local calendar', () => {
  it('keeps Phoenix October 4 at 10:54 PM on October 4 despite UTC being October 5', () => {
    vi.useFakeTimers();
    vi.setSystemTime(new Date('2026-10-05T05:54:00Z'));
    expect(todayInTimezone('America/Phoenix')).toBe('2026-10-04');
    expect(todayInTimezone('UTC')).toBe('2026-10-05');
    const range = rangeForPreset(todayInTimezone('America/Phoenix'), 'last30');
    expect(range).toEqual({ start: '2026-09-05', end: '2026-10-05' });
    expect(displayRange(range)).toBe('2026-09-05 — 2026-10-04');
  });
  it('advances at Phoenix midnight instead of UTC midnight', () => {
    vi.useFakeTimers();
    vi.setSystemTime(new Date('2026-10-05T06:59:59Z'));
    expect(todayInTimezone('America/Phoenix')).toBe('2026-10-04');
    vi.setSystemTime(new Date('2026-10-05T07:00:00Z'));
    expect(todayInTimezone('America/Phoenix')).toBe('2026-10-05');
  });
  it('derives today and every preset from the effective zone at a midnight boundary', () => {
    vi.useFakeTimers();
    vi.setSystemTime(new Date('2026-10-05T06:30:00Z'));
    const phoenix = todayInTimezone('America/Phoenix'); const tokyo = todayInTimezone('Asia/Tokyo');
    expect(phoenix).toBe('2026-10-04'); expect(tokyo).toBe('2026-10-05');
    expect(rangeForPreset(phoenix, 'today')).toEqual({ start: '2026-10-04', end: '2026-10-05' });
    expect(rangeForPreset(tokyo, 'today')).toEqual({ start: '2026-10-05', end: '2026-10-06' });
    expect(rangeForPreset(phoenix, 'week')).toEqual({ start: '2026-09-28', end: '2026-10-05' });
    expect(rangeForPreset(tokyo, 'week')).toEqual({ start: '2026-10-05', end: '2026-10-06' });
    expect(rangeForPreset(phoenix, 'last30')).toEqual({ start: '2026-09-05', end: '2026-10-05' });
    expect(rangeForPreset(tokyo, 'last30')).toEqual({ start: '2026-09-06', end: '2026-10-06' });
    expect(rangeForPreset(phoenix, 'month')).toEqual(rangeForPreset(tokyo, 'month'));
  });
  it('uses the operating system zone instead of a fixed city', () => {
    vi.spyOn(Intl, 'DateTimeFormat').mockReturnValue({ resolvedOptions: () => ({ timeZone: 'Europe/Berlin' }) } as Intl.DateTimeFormat);
    expect(systemTimezone()).toBe('Europe/Berlin');
  });
  it('honors the local date on the other side of UTC and across daylight saving changes', () => {
    vi.useFakeTimers();
    vi.setSystemTime(new Date('2026-10-04T17:00:00Z'));
    expect(todayInTimezone('Asia/Shanghai')).toBe('2026-10-05');
    vi.setSystemTime(new Date('2026-03-08T07:30:00Z'));
    expect(todayInTimezone('America/Los_Angeles')).toBe('2026-03-07');
    vi.setSystemTime(new Date('2026-03-09T07:00:00Z'));
    expect(todayInTimezone('America/Los_Angeles')).toBe('2026-03-09');
  });
});
