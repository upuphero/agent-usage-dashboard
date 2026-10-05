import { useEffect, useState } from 'react';
import { useMutation, useQuery, useQueryClient } from '@tanstack/react-query';
import { useUsageClient } from '../app/UsageContext';
import type { ExportRequest, OverviewQuery, SessionQuery, ScanSummary, UpdateSettingsRequest } from '../api/generated/usage';
import { apiError, assertResponseVersion, normalizeError } from '../api/protocol';

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
  return useMutation({ mutationFn: async (request: ExportRequest) => assertResponseVersion(await client.exportUsage(request)) });
}
export function useSettings(enabled: boolean) {
  const client = useUsageClient(); const cache = useQueryClient();
  const query = useQuery({ queryKey: ['usage', 'settings'], enabled: enabled && !!client.getSettings, queryFn: async () => {
    if (!client.getSettings) throw apiError('UNSUPPORTED_FILTER', '当前客户端没有设置接口。');
    return assertResponseVersion(await client.getSettings());
  } });
  const save = useMutation({ mutationFn: async (request: UpdateSettingsRequest) => {
    if (!client.updateSettings) throw apiError('UNSUPPORTED_FILTER', '当前客户端不能保存设置。');
    return assertResponseVersion(await client.updateSettings(request));
  }, onSuccess: async result => { cache.setQueryData(['usage', 'settings'], result); await cache.invalidateQueries({ queryKey: ['usage'], predicate: value => !['api', 'settings'].includes(String(value.queryKey[1])) }); } });
  const choose = useMutation({ mutationFn: async (providerId: string) => {
    if (!client.chooseProviderDirectory) throw apiError('UNSUPPORTED_FILTER', '当前客户端没有目录选择接口。');
    return assertResponseVersion(await client.chooseProviderDirectory(providerId));
  } });
  return { query, save, choose, canRead: !!client.getSettings, canWrite: !!client.updateSettings, canChoose: !!client.chooseProviderDirectory };
}
