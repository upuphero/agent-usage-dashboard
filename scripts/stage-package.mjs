import { cp, mkdir, readFile, readdir, writeFile } from 'node:fs/promises';
import { resolve } from 'node:path';
import { readLock, projectRoot } from './prepare-ccusage.mjs';

const target = process.argv[2];
await readLock(target); // Reject extra platforms before resolving any input/output paths.
const extension = target === 'x86_64-pc-windows-msvc' ? '.exe' : '.dmg';
const format = extension === '.exe' ? 'nsis' : 'dmg';
const source = resolve(projectRoot, 'target', target, 'release/bundle', format);
const files = (await readdir(source, { withFileTypes: true })).filter(file => file.isFile() && file.name.endsWith(extension));
if (files.length !== 1) throw new Error('Exactly one platform installer is required');
const destination = resolve(projectRoot, 'artifacts', target);
await mkdir(destination, { recursive: true });
await readFile(resolve(destination, 'bundle-manifest.json')); // Installation verification must precede staging.
await cp(resolve(source, files[0].name), resolve(destination, files[0].name), { errorOnExist: true, force: false });
await cp(resolve(projectRoot, 'THIRD_PARTY_NOTICES.md'), resolve(destination, 'THIRD_PARTY_NOTICES.md'));
await writeFile(resolve(destination, 'build-info.json'), JSON.stringify({
  target, appCommit: process.env.GITHUB_SHA, runId: process.env.GITHUB_RUN_ID,
  repository: process.env.GITHUB_REPOSITORY, node: process.version, rust: '1.91.1', pnpm: '9.15.0',
  runnerImage: { os: process.env.ImageOS, version: process.env.ImageVersion },
  signing: target === 'x86_64-pc-windows-msvc' ? 'unsigned' : 'ad-hoc; not Developer ID or notarized',
  uiSmoke: 'unverified', minimumOs: 'unverified', fullDependencyNotices: 'pending',
}, null, 2) + '\n');
