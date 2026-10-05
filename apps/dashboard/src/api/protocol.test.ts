import { describe, expect, it } from 'vitest';
import { API_VERSION } from './generated/usage';
import { apiError, assertApiVersion, normalizeError } from './protocol';

describe('wire errors and version negotiation', () => {
  it('preserves stable errors from object and JSON IPC rejections', () => {
    const error = apiError('PERMISSION_DENIED', '权限不足', true);
    expect(normalizeError(error)).toEqual(error);
    expect(normalizeError(JSON.stringify(error))).toEqual(error);
  });
  it('does not infer an error code from message text or expose raw diagnostics', () => {
    const error = normalizeError(new Error('PERMISSION_DENIED C:/private/path'));
    expect(error).toMatchObject({ apiVersion: API_VERSION, code: 'INTERNAL', retryable: true });
    expect(error.message).not.toContain('private');
    expect(normalizeError({ code: 'FUTURE_ERROR', message: 'secret' }).code).toBe('INTERNAL');
  });
  it('accepts compatible minor versions and rejects incompatible/malformed versions', () => {
    expect(() => assertApiVersion('1.3.0')).not.toThrow();
    for (const version of ['2.0.0', '0.1.0', '1.invalid', '01.0.0']) {
      expect(() => assertApiVersion(version)).toThrow(expect.objectContaining({ code: 'API_VERSION_UNSUPPORTED' }));
    }
  });
});
