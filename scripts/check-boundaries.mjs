import { readFile, readdir } from 'node:fs/promises';
import { resolve, relative } from 'node:path';
import { execFileSync } from 'node:child_process';
const root = resolve(import.meta.dirname, '..');
const graph = JSON.parse(execFileSync('cargo', ['metadata', '--format-version=1', '--no-deps', '--offline'], { cwd: root, encoding: 'utf8' }));
const allowed = {
  'usage-core': new Set(['async-trait', 'chrono', 'chrono-tz', 'rust_decimal', 'serde', 'thiserror']),
  'usage-contracts': new Set(['serde']),
};
for (const pkg of graph.packages) {
  for (const dep of pkg.dependencies.filter(d => d.kind !== 'dev')) {
    if (allowed[pkg.name] && !allowed[pkg.name].has(dep.name)) throw new Error(`${pkg.name} forbidden dependency: ${dep.name}`);
    if (pkg.name === 'usage-adapters' && ['tauri', 'usage-desktop', 'usage-contracts'].includes(dep.name)) throw new Error(`Adapter dependency forbidden: ${dep.name}`);
  }
}
async function files(dir) {
  const result = [];
  for (const item of await readdir(dir, { withFileTypes: true })) {
    const path = resolve(dir, item.name);
    if (item.isDirectory()) result.push(...await files(path)); else if (/\.[jt]sx?$/.test(path)) result.push(path);
  }
  return result;
}
for (const file of await files(resolve(root, 'apps/dashboard/src'))) {
  const path = relative(root, file).replaceAll('\\', '/');
  if (path.includes('/api/transports/tauri/')) continue;
  if (/@tauri-apps\/|__TAURI__|\binvoke\s*\(/.test(await readFile(file, 'utf8'))) throw new Error(`Tauri bridge outside transport: ${path}`);
}
console.log('Dependency boundaries passed.');
