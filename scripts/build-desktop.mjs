import { spawnSync } from 'node:child_process';
import { resolve } from 'node:path';
import { pathToFileURL } from 'node:url';
const root = resolve(import.meta.dirname, '..');
export const nativeTarget = ({ platform, arch }) => platform === 'win32' && arch === 'x64' ? 'x86_64-pc-windows-msvc' : platform === 'darwin' && arch === 'arm64' ? 'aarch64-apple-darwin' : null;
export function buildPlan(argv, host = process) {
  let target; let local = false;
  for (let i = 0; i < argv.length; i++) {
    const arg = argv[i];
    if (arg === '--local' && !local) local = true;
    else if (arg.startsWith('--target=') && target === undefined) target = arg.slice(9);
    else if (arg === '--target' && target === undefined) target = argv[++i] ?? '';
    else throw new Error(`Unsupported or duplicate build argument: ${arg}`);
  }
  const native = nativeTarget(host);
  if (!native) throw new Error('Build requires Windows x64 or Apple Silicon ARM64 with native Node/Rust tooling.');
  target ??= native;
  if (target !== native) throw new Error('Only native matching-target builds are supported; Intel Mac, universal and cross builds are excluded.');
  const steps = [['node', 'scripts/verify-version.mjs'], ['node', 'scripts/generate-contracts.mjs', '--check'], ['node', 'scripts/check-boundaries.mjs'], ['node', 'scripts/prepare-notices.mjs', '--check']];
  if (!local) steps.push(['node', 'scripts/prepare-ccusage.mjs', '--target', target], ['node', 'scripts/verify-sidecar.mjs', '--target', target]);
  steps.push(['pnpm', '--filter', '@usage/desktop', 'exec', 'tauri', 'build', '--target', target, ...(local ? ['--no-bundle', '--config', 'src-tauri/tauri.local.conf.json'] : ['--bundles', target === 'x86_64-pc-windows-msvc' ? 'nsis' : 'dmg']), '--', '--locked']);
  return { target, local, steps };
}
if (process.argv[1] && import.meta.url === pathToFileURL(resolve(process.argv[1])).href) {
  const plan = buildPlan(process.argv.slice(2));
  for (const [tool, ...args] of plan.steps) {
    const command = tool === 'node' ? process.execPath : tool;
    const result = spawnSync(command, args, { cwd: root, stdio: 'inherit', shell: process.platform === 'win32' && tool === 'pnpm', env: { ...process.env, MACOSX_DEPLOYMENT_TARGET: '13.0' } });
    if (result.error) throw result.error;
    if (result.status !== 0) process.exit(result.status ?? 1);
  }
}
