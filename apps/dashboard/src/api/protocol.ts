import { API_VERSION, ERROR_CODES, type ApiError, type ErrorCode } from './generated/usage';

const codes: ReadonlySet<string> = new Set(ERROR_CODES);

export function apiError(code: ErrorCode, message: string, retryable = false): ApiError {
  return { apiVersion: API_VERSION, code, message, retryable };
}

export function normalizeError(reason: unknown): ApiError {
  let candidate = reason;
  if (typeof candidate === 'string') {
    try { candidate = JSON.parse(candidate); } catch { candidate = null; }
  }
  if (candidate && typeof candidate === 'object' && 'code' in candidate &&
    typeof candidate.code === 'string' && codes.has(candidate.code)) {
    const record = candidate as Record<string, unknown>;
    return {
      apiVersion: typeof record.apiVersion === 'string' ? record.apiVersion : API_VERSION,
      code: candidate.code as ErrorCode,
      message: typeof record.message === 'string' ? record.message : '请求未完成。',
      retryable: record.retryable === true,
    };
  }
  // Do not expose bridge errors, paths or infer stable codes from diagnostic text.
  return apiError('INTERNAL', '无法完成请求，请重试或查看来源状态。', true);
}

export function assertApiVersion(version: string): void {
  if (!/^1\.\d+\.\d+(?:[-+][\w.-]+)?$/.test(version)) {
    throw apiError('API_VERSION_UNSUPPORTED', `接口版本 ${version} 不兼容，需要版本 1.x。`);
  }
}

export function assertResponseVersion<T extends { apiVersion: string }>(value: T): T {
  assertApiVersion(value.apiVersion);
  return value;
}
