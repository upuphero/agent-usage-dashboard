import type { UsageClient } from '../api/client';
import { DEMO_SCENARIOS, MockUsageClient, type DemoScenario } from '../api/transports/mock/MockUsageClient';
import { TauriUsageClient } from '../api/transports/tauri/TauriUsageClient';
import { isTauriRuntime } from '../api/transports/tauri/runtime';

export function createClient(search: string): { client: UsageClient; demo: boolean; scenario: DemoScenario } {
  const params = new URLSearchParams(search);
  const requested = params.get('scenario');
  const scenario = DEMO_SCENARIOS.find(value => value === requested) ?? 'partial';
  const desktop = params.get('client') === 'tauri' || (params.get('client') !== 'mock' && isTauriRuntime());
  return { client: desktop ? new TauriUsageClient() : new MockUsageClient(scenario), demo: !desktop, scenario };
}
