import assert from 'node:assert/strict';
import { spawn } from 'node:child_process';
import { readFile, mkdir, mkdtemp, cp, writeFile, rm } from 'node:fs/promises';
import os from 'node:os';
import path from 'node:path';
import { fileURLToPath } from 'node:url';
import { projectRoot, readLock, verifyBinary, parseArguments } from './prepare-ccusage.mjs';

function run(executable, args, home, root, config) {
  return new Promise((resolve, reject) => {
    const env = { HOME: home, USERPROFILE: home, XDG_CONFIG_HOME: home, XDG_CACHE_HOME: home, CLAUDE_CONFIG_DIR: root, NO_COLOR: '1', HTTP_PROXY: 'http://127.0.0.1:9', HTTPS_PROXY: 'http://127.0.0.1:9' };
    for (const name of ['SystemRoot', 'WINDIR']) if (process.env[name]) env[name] = process.env[name];
    const child = spawn(executable, args.includes('--version') ? args : [...args, '--config', config], { cwd: home, env, shell: false, windowsHide: true, stdio: ['ignore', 'pipe', 'pipe'] });
    let size = 0;
    const chunks = [];
    const timer = setTimeout(() => child.kill(), 10_000);
    child.on('error', reject);
    child.stdout.on('data', chunk => { size += chunk.length; if (size > 1024 * 1024) child.kill(); else chunks.push(chunk); });
    child.stderr.on('data', chunk => { size += chunk.length; if (size > 1024 * 1024) child.kill(); });
    child.on('close', code => {
      clearTimeout(timer);
      if (code !== 0 || size > 1024 * 1024) reject(new Error('Sidecar command failed (raw output suppressed)'));
      else resolve(Buffer.concat(chunks).toString('utf8'));
    });
  });
}

export async function verify({ target, binary, architectureOnly = false }) {
  const { entry, lock } = await readLock(target);
  const executable = path.resolve(binary ?? path.join(projectRoot, 'apps/desktop/src-tauri/binaries', entry.resourceName));
  verifyBinary(await readFile(executable), entry, target);
  if (architectureOnly) return { target, architectureVerified: true, runtimeVerified: false };
  const host = process.platform === 'win32' && process.arch === 'x64' ? 'x86_64-pc-windows-msvc' : process.platform === 'darwin' && process.arch === 'arm64' ? 'aarch64-apple-darwin' : null;
  if (host !== target) throw new Error('Runtime verification requires the matching Windows x64 or macOS ARM64 host');
  const temporary = await mkdtemp(path.join(os.tmpdir(), 'ccusage-验证 '));
  try {
    const home = path.join(temporary, 'home');
    const root = path.join(temporary, 'Claude 日志');
    const config = path.join(temporary, 'empty.json');
    await mkdir(home); await writeFile(config, '{}');
    await cp(path.join(projectRoot, 'tests/fixtures/claude-code/logs'), root, { recursive: true });
    // An intentionally hostile auto-discovered config must not alter command semantics.
    await writeFile(path.join(root, 'ccusage.json'), JSON.stringify({ claude: { defaults: { since: '20990101', offline: false, mode: 'display' } } }));
    assert.equal((await run(executable, ['--version'], home, root, config)).trim(), `ccusage ${lock.version}`);
    const collect = async (kind, timezone = 'America/Phoenix') => JSON.parse(await run(executable, ['claude', kind, '--json', '--offline', '--breakdown', '--mode', 'calculate', '--order', 'asc', '--timezone', timezone], home, root, config));
    let daily = await collect('daily');
    const session = await collect('session');
    assert.deepEqual(daily, JSON.parse(await readFile(path.join(projectRoot, 'tests/fixtures/claude-code/daily.json'), 'utf8')));
    assert.deepEqual(session, JSON.parse(await readFile(path.join(projectRoot, 'tests/fixtures/claude-code/session.json'), 'utf8')));
    assert.deepEqual(await collect('daily'), daily, 'repeat scan');
    assert.equal(daily.totals.totalTokens, 515);
    assert.equal(session.sessions.find(s => s.sessionId === 'session-a').totalTokens, 500);
    assert.deepEqual(daily.totals.unpricedModels, ['synthetic-unpriced-model']);
    assert.equal(daily.daily[1].modelBreakdowns.find(m => m.modelName === 'synthetic-unpriced-model').missingPricing, true);
    for (const row of daily.daily) assert.equal(row.totalTokens, row.inputTokens + row.outputTokens + row.cacheCreationTokens + row.cacheReadTokens);
    const log = path.join(root, 'projects/synthetic/session-a.jsonl');
    const original = await readFile(log, 'utf8');
    await writeFile(log, original.replace('"output_tokens":40', '"output_tokens":45'));
    assert.equal((await collect('daily')).totals.totalTokens, 520, 'correction replaces input log value');
    await writeFile(log, original);
    daily = await collect('daily', 'UTC');
    assert.equal(daily.daily.length, 1, 'timezone rebuckets raw timestamps');
    // Repeated hour in the US fall-back transition, plus midnight at a year boundary.
    await writeFile(path.join(root, 'projects/synthetic/dst.jsonl'), [
      ['2026-11-01T05:30:00Z', 'dst-a'], ['2026-11-01T06:30:00Z', 'dst-b'], ['2027-01-01T04:59:00Z', 'year-a'], ['2027-01-01T05:01:00Z', 'year-b'],
    ].map(([timestamp, id]) => JSON.stringify({ timestamp, requestId: id, message: { id, model: 'claude-sonnet-4-20250514', usage: { input_tokens: 1, output_tokens: 1, cache_creation_input_tokens: 0, cache_read_input_tokens: 0 } } })).join('\n'));
    const transitions = await collect('daily', 'America/New_York');
    assert.equal(transitions.daily.find(r => r.date === '2026-11-01').totalTokens, 4);
    assert.equal(transitions.daily.find(r => r.date === '2026-12-31').totalTokens, 2);
    assert.equal(transitions.daily.find(r => r.date === '2027-01-01').totalTokens, 2);
    const emptyRoot = path.join(temporary, 'empty-root');
    await mkdir(path.join(emptyRoot, 'projects'), { recursive: true });
    const empty = JSON.parse(await run(executable, ['claude', 'daily', '--json', '--offline', '--breakdown', '--mode', 'calculate', '--timezone', 'UTC'], home, emptyRoot, config));
    assert.deepEqual(empty.daily, []);
    assert.equal(empty.totals.totalTokens, 0);
    return { target, version: lock.version, architectureVerified: true, runtimeVerified: true, cases: ['daily/session schema', 'empty-cache offline flags and unreachable proxies', 'isolated config', 'repeat scan', 'correction', 'unknown pricing', 'cache buckets', 'cross-midnight session', 'timezone', 'DST/year boundary', 'readable empty source', 'spaces/Unicode paths'], actualOs: { platform: process.platform, release: os.release(), arch: process.arch }, caveat: 'Offline flags/proxies verified; no packet-level network trace or clean-machine/minimum-OS test' };
  } finally { await rm(temporary, { recursive: true, force: true }); }
}

if (process.argv[1] && path.resolve(process.argv[1]) === fileURLToPath(import.meta.url)) {
  try {
    const args = [...process.argv.slice(2)];
    const index = args.indexOf('--architecture-only');
    const architectureOnly = index !== -1;
    if (architectureOnly) args.splice(index, 1);
    const options = parseArguments(args, ['--target', '--binary']);
    console.log(JSON.stringify(await verify({ target: options['--target'], binary: options['--binary'], architectureOnly })));
  } catch (error) { console.error(error.message); process.exitCode = 1; }
}
