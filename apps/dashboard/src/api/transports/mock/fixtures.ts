import {
  API_VERSION, type CostEstimate, type Metric, type ProviderSummary, type SessionItem,
  type TokenMetrics, type UsageAggregate,
} from '../../generated/usage';

export const DEMO_TIMEZONE = 'America/Phoenix';
export const DEMO_DATE = '2026-10-04';
export const DEMO_UPDATED = '2026-10-04T18:42:00Z';
export const DEMO_RANGE = { start: '2026-09-28', end: '2026-10-05' };
export const unavailable = (): Metric<string> => ({ value: null, accuracy: 'unavailable', knownRows: 0, missingRows: 1 });
export const reported = (value: string, accuracy: Metric<string>['accuracy'] = 'exact', missingRows = 0): Metric<string> =>
  ({ value, accuracy, knownRows: 1, missingRows });

function tokens(total: string, input: string, output: string, cache: string, reasoning: string | null): TokenMetrics {
  return {
    total: reported(total), inputUncached: reported(input), outputTotal: reported(output),
    cacheRead: reported(cache), cacheWrite: reported('0'),
    outputReasoning: reasoning === null ? unavailable() : reported(reasoning),
  };
}

function cost(value: string | null, missingModels: string[] = []): CostEstimate {
  return {
    kind: 'api-equivalent-estimate', amountUsd: value === null ? unavailable() : reported(value, 'estimated', missingModels.length),
    pricingVersions: ['demo-prices-v1'], pricingAsOf: null, missingModels,
  };
}

// All totals and costs are authored fixture results, never a second statistics/pricing engine.
export const ALL_USAGE: UsageAggregate = {
  tokens: tokens('1480000', '620000', '260000', '600000', null),
  cost: cost('12.46000000', ['demo-unpriced-model']),
};
export const TODAY_USAGE: UsageAggregate = {
  tokens: tokens('135000', '60000', '25000', '50000', null), cost: cost('1.26000000', ['demo-unpriced-model']),
};
export const MONTH_USAGE: UsageAggregate = {
  tokens: tokens('962000', '400000', '162000', '400000', null), cost: cost('8.46000000', ['demo-unpriced-model']),
};
export const CLAUDE_USAGE: UsageAggregate = {
  tokens: tokens('820000', '310000', '160000', '350000', null), cost: cost('7.82000000'),
};
export const CODEX_USAGE: UsageAggregate = {
  tokens: tokens('560000', '270000', '90000', '200000', '30000'), cost: cost('4.64000000'),
};
export const ANTIGRAVITY_USAGE: UsageAggregate = {
  tokens: tokens('100000', '40000', '10000', '50000', null), cost: cost(null, ['demo-unpriced-model']),
};
export const EMPTY_USAGE: UsageAggregate = {
  tokens: { inputUncached: unavailable(), outputTotal: unavailable(), cacheRead: unavailable(), cacheWrite: unavailable(), outputReasoning: unavailable(), total: { ...reported('0', 'derived'), knownRows: 0 } },
  cost: cost(null),
};

export const PROVIDERS: ProviderSummary[] = [
  {
    providerId: 'ccusage.claude-code', productId: 'claude-code', displayName: 'Claude Code', enabled: true,
    state: 'ready', pathHint: '~/.claude/projects (演示)', lastSuccessAt: DEMO_UPDATED,
    capabilities: { reportKinds: ['daily', 'session'], supportedDimensions: ['day', 'model', 'session'], supportedMetrics: ['input', 'output', 'cache', 'cost'], supportsDateSessionIntersection: false, supportsIncrementalCollection: false, supportsQuota: false },
    coverage: [{ state: 'complete', range: DEMO_RANGE, observedFrom: null, observedUntil: null }], lastScan: null,
  },
  {
    providerId: 'ccusage.codex', productId: 'codex', displayName: 'OpenAI Codex', enabled: true,
    state: 'ready', pathHint: '~/.codex/sessions (演示)', lastSuccessAt: DEMO_UPDATED,
    capabilities: { reportKinds: ['daily', 'session'], supportedDimensions: ['day', 'model', 'session'], supportedMetrics: ['input', 'output', 'cache', 'reasoning', 'cost'], supportsDateSessionIntersection: false, supportsIncrementalCollection: false, supportsQuota: false },
    coverage: [{ state: 'complete', range: DEMO_RANGE, observedFrom: null, observedUntil: null }], lastScan: null,
  },
  {
    providerId: 'ccusage.antigravity', productId: 'antigravity', displayName: 'Antigravity', enabled: true,
    state: 'partial', pathHint: '本地来源 (演示)', lastSuccessAt: DEMO_UPDATED,
    capabilities: { reportKinds: ['daily'], supportedDimensions: ['day'], supportedMetrics: ['input', 'output', 'cache'], supportsDateSessionIntersection: false, supportsIncrementalCollection: false, supportsQuota: false },
    coverage: [{ state: 'partial', range: DEMO_RANGE, observedFrom: null, observedUntil: null }], lastScan: null,
  },
];

export const PROVIDER_USAGE: Record<string, UsageAggregate> = {
  'ccusage.claude-code': CLAUDE_USAGE, 'ccusage.codex': CODEX_USAGE, 'ccusage.antigravity': ANTIGRAVITY_USAGE,
};
export const TODAY_PROVIDER_USAGE: Record<string, UsageAggregate> = {
  'ccusage.claude-code': { tokens: tokens('80000', '30000', '15000', '35000', null), cost: cost('0.79000000') },
  'ccusage.codex': { tokens: tokens('40000', '25000', '8000', '7000', '2000'), cost: cost('0.47000000') },
  'ccusage.antigravity': { tokens: tokens('15000', '5000', '2000', '8000', null), cost: cost(null, ['demo-unpriced-model']) },
};
export const MONTH_PROVIDER_USAGE: Record<string, UsageAggregate> = {
  'ccusage.claude-code': { tokens: tokens('530000', '190000', '100000', '240000', null), cost: cost('5.20000000') },
  'ccusage.codex': { tokens: tokens('370000', '185000', '55000', '130000', '20000'), cost: cost('3.26000000') },
  'ccusage.antigravity': { tokens: tokens('62000', '25000', '7000', '30000', null), cost: cost(null, ['demo-unpriced-model']) },
};
export const DAILY_PROVIDER_TOTALS: Record<string, readonly string[]> = {
  'ccusage.claude-code': ['70000', '120000', '100000', '160000', '120000', '170000', '80000'],
  'ccusage.codex': ['40000', '100000', '50000', '140000', '90000', '100000', '40000'],
  'ccusage.antigravity': ['8000', '16000', '14000', '15000', '18000', '14000', '15000'],
};
export const SEPTEMBER_PROVIDER_TOTALS: Record<string, string> = {
  'ccusage.claude-code': '290000', 'ccusage.codex': '190000', 'ccusage.antigravity': '38000',
};
export const MODEL_USAGE: Record<string, UsageAggregate> = {
  'demo-claude-model': CLAUDE_USAGE, 'demo-codex-model': CODEX_USAGE,
};
export const MODEL_PROVIDER: Record<string, string> = {
  'demo-claude-model': 'ccusage.claude-code', 'demo-codex-model': 'ccusage.codex',
};
export const DAILY_TOTALS = [
  ['2026-09-28', '118000'], ['2026-09-29', '236000'], ['2026-09-30', '164000'],
  ['2026-10-01', '315000'], ['2026-10-02', '228000'], ['2026-10-03', '284000'], ['2026-10-04', '135000'],
] as const;

export const SESSIONS: SessionItem[] = [
  { sessionId: 'demo-session-01', providerId: 'ccusage.claude-code', productId: 'claude-code', sourceDatasetId: 'demo-dataset-claude', originDeviceId: 'demo-local-device', modelId: 'demo-claude-model', modelVendor: 'Anthropic', startedAt: null, lastActivityAt: '2026-10-04T17:58:00Z', usage: CLAUDE_USAGE },
  { sessionId: 'demo-session-02', providerId: 'ccusage.codex', productId: 'codex', sourceDatasetId: 'demo-dataset-codex', originDeviceId: 'demo-local-device', modelId: 'demo-codex-model', modelVendor: 'OpenAI', startedAt: '2026-09-28T14:20:00Z', lastActivityAt: '2026-10-03T21:14:00Z', usage: CODEX_USAGE },
];
export const DEMO_API_INFO = {
  apiVersion: API_VERSION, appVersion: '0.0.2',
  capabilities: ['overview', 'sessions', 'scan-polling', 'export-json-full-history', 'export-csv', 'demo-data', 'settings-read', 'settings-write', 'source-directory-selection'],
};
