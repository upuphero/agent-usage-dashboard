// Local verification only; never creates a Release or publishes assets.
import { readFile, readdir, stat, writeFile, mkdir } from 'node:fs/promises';
import { createHash } from 'node:crypto';
import { resolve, basename } from 'node:path';
const root = resolve(import.meta.dirname, '..');
const folder = resolve(root, process.argv[2] ?? 'artifacts');
const version = JSON.parse(await readFile(resolve(root, 'package.json'), 'utf8')).version;
async function walk(dir) { const paths = []; for (const item of await readdir(dir, { withFileTypes: true })) { const path = resolve(dir, item.name); if (item.isDirectory()) paths.push(...await walk(path)); else paths.push(path); } return paths; }
const files = (await walk(folder)).filter(p => /\.(exe|dmg)$/.test(p));
const windows = files.filter(p => p.endsWith('.exe')); const mac = files.filter(p => p.endsWith('.dmg'));
if (windows.length !== 1 || mac.length !== 1) throw new Error('Require exactly one Windows x64 NSIS exe and one macOS ARM64 dmg.');
const report = [];
for (const file of [...windows, ...mac]) {
  if (!basename(file).includes(version)) throw new Error(`Version not in artifact name: ${basename(file)}`);
  if (!file.includes(file.endsWith('.exe') ? 'x64' : 'aarch64')) throw new Error(`Architecture marker missing: ${basename(file)}`);
  const size = (await stat(file)).size;
  if (size > 40 * 1048576 && process.env.ALLOW_SIZE_OVER_BUDGET !== '1') throw new Error('Installer exceeds 40 MiB; document explanation before overriding.');
  report.push({ file: basename(file), bytes: size, sha256: createHash('sha256').update(await readFile(file)).digest('hex'), signingStatus: 'unverified' });
}
await mkdir(folder, { recursive: true });
await writeFile(resolve(folder, 'SHA256SUMS'), report.map(r => `${r.sha256}  ${r.file}`).join('\n') + '\n');
await writeFile(resolve(folder, 'size-report.json'), JSON.stringify({ version, artifacts: report }, null, 2) + '\n');
console.log('Both platform artifacts found; size/hash report written. Architecture, signing and installed sidecar smoke still require platform checks.');
