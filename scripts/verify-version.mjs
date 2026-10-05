import { readFile } from 'node:fs/promises';
import { resolve } from 'node:path';
const root = resolve(import.meta.dirname, '..');
const json = async path => JSON.parse(await readFile(resolve(root, path), 'utf8'));
const version = (await json('package.json')).version;
for (const path of ['apps/dashboard/package.json', 'apps/desktop/package.json', 'apps/desktop/src-tauri/tauri.conf.json']) {
  if ((await json(path)).version !== version) throw new Error(`Version mismatch: ${path}`);
}
const cargo = await readFile(resolve(root, 'Cargo.toml'), 'utf8');
if (!cargo.includes(`version = "${version}"`)) throw new Error('Workspace version mismatch');
const tag = process.argv.find(a => a.startsWith('--tag='))?.slice(6);
if (tag && tag !== `v${version}`) throw new Error('Tag/version mismatch');
console.log(`Version ${version} passed.`);
