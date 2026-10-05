import { createContext, useContext, type ReactNode } from 'react';
import type { UsageClient } from '../api/client';

const UsageContext = createContext<UsageClient | null>(null);
export function UsageProvider({ client, children }: { client: UsageClient; children: ReactNode }) {
  return <UsageContext.Provider value={client}>{children}</UsageContext.Provider>;
}
export function useUsageClient(): UsageClient {
  const client = useContext(UsageContext);
  if (!client) throw new Error('UsageProvider is required');
  return client;
}
