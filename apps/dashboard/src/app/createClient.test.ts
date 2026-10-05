import { describe, expect, it } from 'vitest';
import { createClient } from './createClient';
import { MockUsageClient } from '../api/transports/mock/MockUsageClient';
import { TauriUsageClient } from '../api/transports/tauri/TauriUsageClient';

describe('entry point injection', () => {
  it('runs ordinary browsers with a labelled Mock client', () => {
    const result = createClient('');
    expect(result.demo).toBe(true);
    expect(result.client).toBeInstanceOf(MockUsageClient);
    expect(result.scenario).toBe('partial');
  });
  it('selects explicit demo scenarios and normalizes unknown demo names', () => {
    expect(createClient('?scenario=stale').scenario).toBe('stale');
    expect(createClient('?scenario=not-a-scenario').scenario).toBe('partial');
  });
  it('never silently falls back to Mock when desktop transport is explicitly selected', async () => {
    const result = createClient('?client=tauri');
    expect(result.demo).toBe(false);
    expect(result.client).toBeInstanceOf(TauriUsageClient);
    await expect(result.client.getApiInfo()).rejects.toMatchObject({ code: 'INTERNAL' });
  });
  it('rejects an incompatible demo API before business queries', async () => {
    const client = new MockUsageClient('version-mismatch', 0);
    await expect(client.getApiInfo()).rejects.toMatchObject({ code: 'API_VERSION_UNSUPPORTED' });
    await expect(client.listProviders()).rejects.toMatchObject({ code: 'API_VERSION_UNSUPPORTED' });
  });
});
