// Checks staged/installed binaries or the actual .app. It does not install NSIS/DMG or certify signatures.
import { readFile, stat, mkdir, writeFile } from 'node:fs/promises';
import { createHash } from 'node:crypto';
import { resolve, join, basename } from 'node:path';
import { pathToFileURL } from 'node:url';
import { readLock, verifyBinary, projectRoot } from './prepare-ccusage.mjs';
import { verify } from './verify-sidecar.mjs';
export function verifyMainArchitecture(bytes, target) {
  if (target === 'x86_64-pc-windows-msvc') {
    if (bytes.length < 64 || bytes.readUInt16LE(0) !== 0x5a4d) throw new Error('Main binary is not PE');
    const offset = bytes.readUInt32LE(0x3c);
    if (offset < 64 || offset + 26 > bytes.length || bytes.readUInt32LE(offset) !== 0x00004550 || bytes.readUInt16LE(offset + 4) !== 0x8664 || bytes.readUInt16LE(offset + 24) !== 0x20b) throw new Error('Main binary must be Windows AMD64 PE32+');
  } else if (target === 'aarch64-apple-darwin') {
    if (bytes.length < 32 || bytes.readUInt32LE(0) !== 0xfeedfacf || bytes.readUInt32LE(4) !== 0x0100000c) throw new Error('Main binary must be thin ARM64 Mach-O; Intel/universal builds are excluded');
  } else throw new Error('Unsupported bundle target');
}
export async function verifyBundle({ target, root, kind = 'staging', output }) {
  if (!['staging', 'installed', 'app-bundle', 'portable'].includes(kind)) throw new Error('Unsupported verification kind');
  if (kind === 'portable' && target !== 'x86_64-pc-windows-msvc') throw new Error('Portable ZIP is Windows x64 only');
  const { entry, lock } = await readLock(target);
  const config = JSON.parse(await readFile(join(projectRoot, 'apps/desktop/src-tauri/tauri.conf.json'), 'utf8'));
  const directory = resolve(root);
  let executable = target === 'x86_64-pc-windows-msvc' ? 'usage-desktop.exe' : 'usage-desktop';
  let binaryDir = directory;
  if (kind === 'app-bundle') {
    if (target !== 'aarch64-apple-darwin') throw new Error('Only macOS uses app-bundle verification');
    const plist = await readFile(join(directory, 'Contents/Info.plist'), 'utf8');
    const name = /<key>CFBundleExecutable<\/key>\s*<string>([^<]+)<\/string>/.exec(plist)?.[1];
    if (!name || basename(name) !== name || name.includes('\\')) throw new Error('Invalid app bundle executable');
    executable = name; binaryDir = join(directory, 'Contents/MacOS');
  }
  const mainPath = join(binaryDir, executable); const sidecarPath = join(binaryDir, target === 'x86_64-pc-windows-msvc' ? 'ccusage.exe' : 'ccusage');
  const main = await readFile(mainPath); verifyMainArchitecture(main, target);
  const sidecar = await readFile(sidecarPath); verifyBinary(sidecar, entry, target);
  if (target === 'aarch64-apple-darwin' && process.platform === 'darwin') {
    for (const file of [mainPath, sidecarPath]) if (((await stat(file)).mode & 0o111) === 0) throw new Error('Bundle executable permission missing');
  }
  const runtime = await verify({ target, binary: sidecarPath });
  const hash = bytes => createHash('sha256').update(bytes).digest('hex');
  const report = { version: config.version, appCommit: process.env.GITHUB_SHA ?? null, target, kind, architectureVerified: true, ccusageVersion: lock.version, sourceCommit: lock.sourceCommit, main: { filename: executable, bytes: main.length, sha256: hash(main) }, sidecar: { filename: basename(sidecarPath), bytes: sidecar.length, sha256: hash(sidecar) }, runtime, signingStatus: 'unverified', installationSmoke: kind === 'staging' ? 'unverified' : 'installed-sidecar-fixture', minimumOs: 'unverified', runnerImage: { os: process.env.ImageOS ?? null, version: process.env.ImageVersion ?? null } };
  if (output) { await mkdir(resolve(output, '..'), { recursive: true }); await writeFile(output, JSON.stringify(report, null, 2) + '\n'); }
  return report;
}
if (process.argv[1] && import.meta.url === pathToFileURL(resolve(process.argv[1])).href) {
  const args = Object.fromEntries(process.argv.slice(2).map(arg => { const match = /^--(target|root|kind|output)=(.+)$/.exec(arg); if (!match) throw new Error('Use --target= --root= --kind= --output='); return [match[1], match[2]]; }));
  if (!args.target || !args.root) throw new Error('Bundle target/root required');
  console.log(JSON.stringify(await verifyBundle(args)));
}
