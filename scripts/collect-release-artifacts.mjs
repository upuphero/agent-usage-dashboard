// Verification and Actions artifact collection only; never creates or updates a Release.
import { readFile, readdir, stat, writeFile } from 'node:fs/promises';
import { createHash } from 'node:crypto';
import { resolve, basename, dirname, relative } from 'node:path';
import { pathToFileURL } from 'node:url';
import { readLock, projectRoot } from './prepare-ccusage.mjs';

export function verifyEvidence(bundle, build, { target, version, commit, entry }) {
  const kind = target === 'x86_64-pc-windows-msvc' ? 'installed' : 'app-bundle';
  if (!/^[a-f0-9]{40}$/.test(commit ?? '') || build.appCommit !== commit || bundle.appCommit !== commit) throw new Error('Artifacts must match the current commit');
  if (build.target !== target || bundle.target !== target || bundle.version !== version || bundle.kind !== kind) throw new Error('Artifact version/target/installation mismatch');
  if (bundle.architectureVerified !== true || bundle.runtime?.runtimeVerified !== true || bundle.runtime?.target !== target || bundle.installationSmoke !== 'installed-sidecar-fixture') throw new Error('Native installed binary evidence is incomplete');
  if (bundle.sidecar?.sha256 !== entry.binarySha256 || bundle.sidecar?.bytes !== entry.binaryBytes) throw new Error('Installed sidecar must match the fixed release lock');
}

async function walk(dir) {
  const paths = [];
  for (const item of await readdir(dir, { withFileTypes: true })) {
    const path = resolve(dir, item.name);
    if (item.isSymbolicLink()) throw new Error('Artifact links are unsupported');
    if (item.isDirectory()) paths.push(...await walk(path));
    else if (item.isFile()) paths.push(path);
  }
  return paths;
}
export async function collect(folder, commit = process.env.GITHUB_SHA) {
  folder = resolve(folder);
  const version = JSON.parse(await readFile(resolve(projectRoot, 'package.json'), 'utf8')).version;
  const files = (await walk(folder)).filter(file => /\.(exe|dmg)$/.test(file));
  const windows = files.filter(file => file.endsWith('.exe'));
  const mac = files.filter(file => file.endsWith('.dmg'));
  if (windows.length !== 1 || mac.length !== 1) throw new Error('Require exactly one Windows x64 NSIS exe and one macOS ARM64 dmg');
  const report = [];
  for (const file of [...windows, ...mac]) {
    const target = file.endsWith('.exe') ? 'x86_64-pc-windows-msvc' : 'aarch64-apple-darwin';
    const { entry } = await readLock(target);
    const [bundle, build] = await Promise.all(['bundle-manifest.json', 'build-info.json'].map(async name => JSON.parse(await readFile(resolve(dirname(file), name), 'utf8'))));
    verifyEvidence(bundle, build, { target, version, commit, entry });
    if (!basename(file).includes(version) || !basename(file).includes(file.endsWith('.exe') ? 'x64' : 'aarch64')) throw new Error('Installer name must identify the version and target');
    const size = (await stat(file)).size;
    if (size === 0 || size > 40 * 1048576) throw new Error('Installer exceeds the 40 MiB budget or is empty');
    report.push({ file: relative(folder, file).split('\\').join('/'), target, bytes: size, sha256: createHash('sha256').update(await readFile(file)).digest('hex'), signing: build.signing });
  }
  await writeFile(resolve(folder, 'SHA256SUMS'), report.map(item => `${item.sha256}  ${item.file}`).join('\n') + '\n');
  await writeFile(resolve(folder, 'size-report.json'), JSON.stringify({ version, appCommit: commit, artifacts: report }, null, 2) + '\n');
  if (process.env.GITHUB_STEP_SUMMARY) await writeFile(process.env.GITHUB_STEP_SUMMARY, `Both native installers verified at ${commit}.\n\n| Target | Size | Signing |\n| --- | ---: | --- |\n${report.map(item => `| ${item.target} | ${(item.bytes / 1048576).toFixed(2)} MiB | ${item.signing} |`).join('\n')}\n\nDownload desktop-installers-${commit} (retained for one day). UI, minimum OS and full transitive notices remain pending. This workflow does not publish a Release.\n`, { flag: 'a' });
  return report;
}
if (process.argv[1] && import.meta.url === pathToFileURL(resolve(process.argv[1])).href) {
  console.log(JSON.stringify(await collect(resolve(projectRoot, process.argv[2] ?? 'artifacts'))));
}
