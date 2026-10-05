import type { ScanSummary } from '../generated/usage';
import { apiError } from '../protocol';

export const isActiveScan = (scan: ScanSummary) => scan.state === 'queued' || scan.state === 'running';
export const delay = (ms: number) => new Promise<void>(resolve => setTimeout(resolve, ms));

/** getScan is a long poll in both clients. Timers and IPC polling stay in transports.
 * The UI shows a pending scan while awaiting the terminal, unchanged wire DTO.
 * A timeout never cancels the backend job; the UI can still cancel or reconnect.
 */
export async function waitForScan(
  read: () => Promise<ScanSummary>, intervalMs: number, maxReads = 800,
): Promise<ScanSummary> {
  for (let attempt = 0; attempt < maxReads; attempt++) {
    const scan = await read();
    if (!isActiveScan(scan)) return scan;
    await delay(intervalMs);
  }
  throw apiError('TIMEOUT', '等待扫描结果超时。已有数据已保留，可继续等待或取消扫描。', true);
}
