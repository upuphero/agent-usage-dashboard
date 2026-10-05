import { describe, expect, it } from 'vitest';
import { TauriUsageClient, type CommandInvoker } from './TauriUsageClient';
import { API_VERSION, COMMANDS } from '../../generated/usage';
import { DEMO_API_INFO, DEMO_RANGE, DEMO_TIMEZONE } from '../mock/fixtures';

describe('native dialog language negotiation', () => {
  for (const supported of [false, true]) {
    it(supported ? 'sends the selected language to a capable host' : 'keeps legacy invoke arguments unchanged', async () => {
      const calls: Array<{ command: string; args?: Record<string, unknown> }> = [];
      const send: CommandInvoker = async <T,>(command: string, args?: Record<string, unknown>) => {
        calls.push({ command, args });
        if (command === COMMANDS.getApiInfo) return { ...DEMO_API_INFO, capabilities: [...DEMO_API_INFO.capabilities, ...(supported ? ['localized-dialogs'] : [])] } as T;
        return { apiVersion: API_VERSION } as T;
      };
      const client = new TauriUsageClient(send);
      await client.chooseProviderDirectory('ccusage.codex', 'en');
      const request = { format: 'csv' as const, query: { range: DEMO_RANGE, timezone: DEMO_TIMEZONE, providerIds: [], modelIds: [], bucket: 'day' as const } };
      await client.exportUsage(request, 'en');
      expect(calls.find(call => call.command === COMMANDS.chooseProviderDirectory)?.args).toEqual({ request: { providerId: 'ccusage.codex' }, ...(supported ? { language: 'en' } : {}) });
      expect(calls.find(call => call.command === COMMANDS.exportUsage)?.args).toEqual({ request, ...(supported ? { language: 'en' } : {}) });
    });
  }
});
