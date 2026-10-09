import { useEffect, useState } from 'react';
import { useMutation, useQuery, useQueryClient } from '@tanstack/react-query';
import type { QueryClient } from '@tanstack/react-query';
import type { UsageEvent } from '../api/client';
import { useUsageClient } from '../app/UsageContext';
import type { ExportRequest, OverviewQuery, SessionQuery, ScanSummary, UpdateSettingsRequest, UpdateAutoCollectionRequest, TimezoneStatus, UpdateTimezoneRequest } from '../api/generated/usage';
import { apiError, assertResponseVersion, normalizeError } from '../api/protocol';
import { useI18n } from '../i18n/I18nContext';

export function useApiInfo() {
  const client = useUsageClient();
  return useQuery({ queryKey: ['usage', 'api'], queryFn: async () => assertResponseVersion(await client.getApiInfo()), staleTime: Infinity });
}
export function useProviders(enabled: boolean) {
  const client = useUsageClient();
  const cache = useQueryClient();
  const query = useQuery({ queryKey: ['usage', 'providers'], queryFn: () => client.listProviders(), enabled,
    refetchInterval: value => value.state.data?.some(provider => provider.state === 'scanning') ? 750 : false });
  useEffect(() => {
    if (query.data) void cache.invalidateQueries({ queryKey: ['usage'], predicate: value => ['overview', 'sessions'].includes(String(value.queryKey[1])) });
  }, [cache, query.data]);
  return query;
}
export function useOverview(query: OverviewQuery, enabled: boolean) {
  const client = useUsageClient();
  return useQuery({ queryKey: ['usage', 'overview', query], queryFn: async () => assertResponseVersion(await client.getOverview(query)), enabled });
}
export function useSessions(query: SessionQuery, enabled: boolean) {
  const client = useUsageClient();
  return useQuery({ queryKey: ['usage', 'sessions', query], queryFn: async () => assertResponseVersion(await client.listSessions(query)), enabled });
}

export function useScan(timezone: string) {
  const client = useUsageClient();
  const cache = useQueryClient();
  const [job, setJob] = useState<{ jobId: string; providerId: string } | null>(null);
  const [outcome, setOutcome] = useState<ScanSummary | null>(null);
  const refresh = () => cache.invalidateQueries({ queryKey: ['usage'], predicate: query => query.queryKey[1] !== 'api' });
  const finish = async (summary: ScanSummary) => { setOutcome(summary); setJob(null); await refresh(); return summary; };
  const monitor = useMutation({
    mutationFn: async (input: { providerId: string; jobId?: string }) => {
      setOutcome(null);
      const { jobId } = input.jobId ? { jobId: input.jobId } : await client.startScan({ providerId: input.providerId, timezone });
      setJob({ jobId, providerId: input.providerId });
      void refresh();
      // Client long polling owns all timers. No SDK, event names or polling loops in hooks.
      return finish(assertResponseVersion(await client.getScan(jobId)));
    },
  });
  const cancel = useMutation({ mutationFn: async () => {
    if (!job) return;
    await client.cancelScan(job.jobId);
    if (!monitor.isPending) await finish(assertResponseVersion(await client.getScan(job.jobId)));
  } });
  return {
    job, outcome, pending: monitor.isPending, cancelling: cancel.isPending,
    error: monitor.error ? normalizeError(monitor.error) : cancel.error ? normalizeError(cancel.error) : outcome?.error ?? null,
    start: (providerId: string) => monitor.mutate({ providerId }),
    resume: () => { if (job) monitor.mutate(job); }, cancel: () => cancel.mutate(),
  };
}

export function useExport() {
  const client = useUsageClient();
  const { language } = useI18n();
  return useMutation({ mutationFn: async (request: ExportRequest) => assertResponseVersion(await client.exportUsage(request, language)) });
}
export function useSettings(enabled: boolean) {
  const client = useUsageClient(); const cache = useQueryClient();
  const { language } = useI18n();
  const query = useQuery({ queryKey: ['usage', 'settings'], enabled: enabled && !!client.getSettings, queryFn: () => readRevisioned(cache, async () => {
    if (!client.getSettings) throw apiError('UNSUPPORTED_FILTER', '当前客户端没有设置接口。');
    return assertResponseVersion(await client.getSettings());
  }) });
  const save = useMutation({ mutationFn: async (request: UpdateSettingsRequest) => {
    if (!client.updateSettings) throw apiError('UNSUPPORTED_FILTER', '当前客户端不能保存设置。');
    return assertResponseVersion(await client.updateSettings(request));
  }, onSuccess: async result => { cache.setQueryData(['usage', 'settings'], result); await cache.invalidateQueries({ queryKey: ['usage'], predicate: value => !['api', 'settings'].includes(String(value.queryKey[1])) }); } });
  const choose = useMutation({ mutationFn: async (providerId: string) => {
    if (!client.chooseProviderDirectory) throw apiError('UNSUPPORTED_FILTER', '当前客户端没有目录选择接口。');
    return assertResponseVersion(await client.chooseProviderDirectory(providerId, language));
  } });
  return { query, save, choose, canRead: !!client.getSettings, canWrite: !!client.updateSettings, canChoose: !!client.chooseProviderDirectory };
}
export function useAutoCollection(enabled: boolean) {
  const client = useUsageClient(); const cache = useQueryClient();
  const query = useQuery({ queryKey: ['usage', 'auto'], enabled: enabled && !!client.getAutoCollection,
    queryFn: () => readRevisioned(cache, async () => { if (!client.getAutoCollection) throw apiError('UNSUPPORTED_FILTER', '当前服务未提供这项设置能力。'); return assertResponseVersion(await client.getAutoCollection()); }) });
  const save = useMutation({ mutationFn: async (request: UpdateAutoCollectionRequest) => {
    if (!client.updateAutoCollection) throw apiError('UNSUPPORTED_FILTER', '当前服务未提供这项设置能力。');
    return assertResponseVersion(await client.updateAutoCollection(request));
  }, onSuccess: async result => { cache.setQueryData(['usage', 'auto'], result); await cache.invalidateQueries({ queryKey: ['usage'], predicate: value => ['settings', 'timezone'].includes(String(value.queryKey[1])) }); } });
  const cancel = useMutation({ mutationFn: (jobId: string) => client.cancelScan(jobId), onSuccess: () => cache.invalidateQueries({ queryKey: ['usage', 'auto'] }) });
  return { query, save, cancel };
}
const sequenceOf = (status: TimezoneStatus | undefined) => { try { return status ? BigInt(status.sequence) : -1n; } catch { return -1n; } };
type Revisioned = { revision: string; timezone: string };
const disagrees = (data: Revisioned, status: TimezoneStatus) => data.revision !== status.revision || data.timezone !== status.effectiveTimezone;
/** Settings or Automatic collection data cached before the first status that disagrees with the host's shared revision or zone. */
const predatesStatus = (cache: QueryClient, next: TimezoneStatus) => ['settings', 'auto'].some(name => {
  const cached = cache.getQueryData<Revisioned>(['usage', name]);
  return !!cached && disagrees(cached, next);
});
/** Settings or Automatic collection reads for their queries. A status accepted while a read was in flight may be newer than its
 * response; background resync never refreshes Settings (and Automatic collection only periodically), so a response that disagrees
 * with that status is read again. A read with no newer status accepted meanwhile is kept, so each extra read needs a newer status. */
export async function readRevisioned<T extends Revisioned>(cache: QueryClient, read: () => Promise<T>): Promise<T> {
  for (;;) {
    const before = sequenceOf(cache.getQueryData<TimezoneStatus>(['usage', 'timezone']));
    const result = await read();
    const latest = cache.getQueryData<TimezoneStatus>(['usage', 'timezone']);
    if (!latest || sequenceOf(latest) <= before || !disagrees(result, latest)) return result;
  }
}
/** Rebuilt data or a new shared revision makes zone-dependent views and revision-guarded editors stale (never 'api').
 * The first accepted status has nothing to compare with, so it reconciles caches loaded earlier against the host instead. */
function invalidateTimezoneDependents(cache: QueryClient, previous: TimezoneStatus | undefined, next: TimezoneStatus) {
  const stale = previous ? previous.effectiveTimezone !== next.effectiveTimezone || previous.revision !== next.revision || previous.mode !== next.mode || (previous.rebuild !== 'idle' && next.rebuild === 'idle') : predatesStatus(cache, next);
  if (!stale) return;
  void cache.invalidateQueries({ queryKey: ['usage'], predicate: query => ['settings', 'providers', 'overview', 'sessions', 'auto'].includes(String(query.queryKey[1])) });
}
/** Applies host timezone status in sequence order; older responses/events are ignored. Returns whether it was stored. */
export function acceptTimezoneStatus(cache: QueryClient, status: TimezoneStatus): boolean {
  const previous = cache.getQueryData<TimezoneStatus>(['usage', 'timezone']);
  if (previous && sequenceOf(status) <= sequenceOf(previous)) return false;
  cache.setQueryData(['usage', 'timezone'], status);
  invalidateTimezoneDependents(cache, previous, status);
  return true;
}
/** The same rules for getTimezone responses, which the query stores itself. Returns the status to keep. */
export function resolveFetchedTimezone(cache: QueryClient, fetched: TimezoneStatus): TimezoneStatus {
  // An event may have stored a newer status while this read was in flight; never regress.
  const cached = cache.getQueryData<TimezoneStatus>(['usage', 'timezone']);
  if (cached && sequenceOf(cached) >= sequenceOf(fetched)) return cached;
  invalidateTimezoneDependents(cache, cached, fetched);
  return fetched;
}
export function useTimezone(enabled: boolean) {
  const client = useUsageClient(); const cache = useQueryClient();
  const query = useQuery({ queryKey: ['usage', 'timezone'], enabled: enabled && !!client.getTimezone, queryFn: async () => {
    if (!client.getTimezone) throw apiError('UNSUPPORTED_FILTER', '当前服务未提供这项设置能力。');
    return resolveFetchedTimezone(cache, assertResponseVersion(await client.getTimezone()));
  } });
  const save = useMutation({ mutationFn: async (request: UpdateTimezoneRequest) => {
    if (!client.updateTimezone) throw apiError('UNSUPPORTED_FILTER', '当前服务未提供这项设置能力。');
    return assertResponseVersion(await client.updateTimezone(request));
  }, onSuccess: result => { acceptTimezoneStatus(cache, result); } });
  const cancel = useMutation({ mutationFn: (jobId: string) => client.cancelScan(jobId), onSuccess: () => cache.invalidateQueries({ queryKey: ['usage', 'timezone'] }) });
  return { query, save, cancel, supported: !!client.getTimezone && !!client.updateTimezone };
}
/** One subscription per app. Transport owns fallback polling and reconnect/focus detection. */
export function useBackgroundUsage(enabled: boolean) {
  const client = useUsageClient(); const cache = useQueryClient();
  useEffect(() => {
    if (!enabled || !client.subscribeUsage) return;
    let disposed = false; let remove: (() => void) | undefined;
    void client.subscribeUsage(event => {
      if (disposed) return;
      applyBackgroundEvent(cache, event);
    }).then(unsubscribe => { if (disposed) unsubscribe(); else remove = unsubscribe; }).catch(() => {
      if (!disposed) void cache.invalidateQueries({ queryKey: ['usage', 'providers'] });
    });
    return () => { disposed = true; remove?.(); };
  }, [enabled, client, cache]);
}
export function applyBackgroundEvent(cache: QueryClient, event: UsageEvent) {
  if (event.kind === 'auto') { cache.setQueryData(['usage', 'auto'], event.status); return; }
  if (event.kind === 'timezone') { acceptTimezoneStatus(cache, event.status); return; }
  void cache.invalidateQueries({ queryKey: ['usage'], predicate: query => !['api', 'settings'].includes(String(query.queryKey[1])) });
}
