import { createHash, timingSafeEqual } from 'node:crypto';
import { readFile, writeFile, mkdir, rename, chmod, rm } from 'node:fs/promises';
import path from 'node:path';
import { fileURLToPath } from 'node:url';
import { gunzipSync } from 'node:zlib';

export const projectRoot = path.resolve(path.dirname(fileURLToPath(import.meta.url)), '..');
export const targets = Object.freeze({
  'x86_64-pc-windows-msvc': { package: '@ccusage/ccusage-win32-x64', binaryPath: 'package/bin/ccusage.exe', resourceName: 'ccusage-x86_64-pc-windows-msvc.exe' },
  'aarch64-apple-darwin': { package: '@ccusage/ccusage-darwin-arm64', binaryPath: 'package/bin/ccusage', resourceName: 'ccusage-aarch64-apple-darwin' },
});

export async function readLock(target) {
  if (!Object.hasOwn(targets, target)) throw new Error('Unsupported target; only Windows x64 and macOS ARM64 are supported');
  const lock = JSON.parse(await readFile(path.join(projectRoot, 'ccusage.lock.json'), 'utf8'));
  const entry = lock.targets[target];
  if (lock.lockVersion !== 1 || !entry || entry.package !== targets[target].package || entry.binaryPath !== targets[target].binaryPath || entry.resourceName !== targets[target].resourceName || !/^\d+\.\d+\.\d+$/.test(lock.version) || !/^[a-f0-9]{64}$/.test(entry.binarySha256)) throw new Error('Invalid sidecar lock');
  const expectedUrl = `https://registry.npmjs.org/${entry.package}/-/${entry.package.split('/')[1]}-${lock.version}.tgz`;
  if (entry.tarballUrl !== expectedUrl) throw new Error('Unexpected release URL');
  return { lock, entry };
}

export function verifyIntegrity(bytes, integrity) {
  if (!/^sha512-[A-Za-z0-9+/]+={0,2}$/.test(integrity)) throw new Error('Invalid locked integrity');
  const expected = Buffer.from(integrity.slice(7), 'base64');
  const actual = createHash('sha512').update(bytes).digest();
  if (expected.length !== actual.length || !timingSafeEqual(expected, actual)) throw new Error('Tarball integrity mismatch');
}

// Only copy one verified regular file into memory. Never extract archive paths to disk.
export function extractBinary(tgz, binaryPath) {
  if (tgz.length > 16 * 1024 * 1024) throw new Error('Tarball size limit exceeded');
  const tar = gunzipSync(tgz, { maxOutputLength: 32 * 1024 * 1024 });
  const seen = new Set();
  let binary;
  for (let offset = 0; offset + 512 <= tar.length;) {
    const header = tar.subarray(offset, offset + 512);
    if (header.every(b => b === 0)) break;
    const string = (start, end) => header.subarray(start, end).toString('utf8').split('\0')[0];
    const name = [string(345, 500), string(0, 100)].filter(Boolean).join('/');
    if (name.includes('\\') || name.includes(':') || name.startsWith('/') || name.split('/').some(s => s === '..' || s === '.')) throw new Error('Unsafe archive path');
    const sizeText = string(124, 136).trim();
    if (!/^[0-7]+$/.test(sizeText)) throw new Error('Invalid tar size');
    const size = Number.parseInt(sizeText, 8);
    let checksum = 0;
    for (let i = 0; i < 512; i++) checksum += i >= 148 && i < 156 ? 32 : header[i];
    if (checksum !== Number.parseInt(string(148, 156).trim(), 8)) throw new Error('Invalid tar checksum');
    const type = header[156];
    if (![0, 48, 53].includes(type) || (type === 53 && size !== 0)) throw new Error('Archive links or extensions are unsupported');
    if (seen.has(name)) throw new Error('Duplicate archive entry');
    seen.add(name);
    if (offset + 512 + size > tar.length) throw new Error('Truncated tar entry');
    if (name === binaryPath) {
      if (type === 53 || size === 0) throw new Error('Executable is not a regular file');
      binary = Buffer.from(tar.subarray(offset + 512, offset + 512 + size));
    }
    offset += 512 + Math.ceil(size / 512) * 512;
  }
  if (!binary) throw new Error('Locked executable is absent');
  return binary;
}

export function verifyBinary(bytes, entry, target) {
  if (createHash('sha256').update(bytes).digest('hex') !== entry.binarySha256) throw new Error('Executable SHA-256 mismatch');
  if (bytes.length !== entry.binaryBytes) throw new Error('Executable size mismatch');
  if (target === 'x86_64-pc-windows-msvc') {
    if (bytes.length < 64 || bytes.toString('ascii', 0, 2) !== 'MZ') throw new Error('Expected PE executable');
    const pe = bytes.readUInt32LE(60);
    if (pe + 26 > bytes.length || bytes.readUInt32LE(pe) !== 0x4550 || bytes.readUInt16LE(pe + 4) !== 0x8664 || bytes.readUInt16LE(pe + 24) !== 0x20b) throw new Error('Expected Windows x64 PE32+');
  } else if (target === 'aarch64-apple-darwin') {
    if (bytes.length < 32 || bytes.readUInt32LE(0) !== 0xfeedfacf || bytes.readUInt32LE(4) !== 0x100000c || bytes.readUInt32LE(12) !== 2) throw new Error('Expected ARM64 Mach-O executable');
  } else throw new Error('Unsupported target');
}

async function download(url) {
  const response = await fetch(url, { redirect: 'error', signal: AbortSignal.timeout(60_000) });
  if (!response.ok || Number(response.headers.get('content-length')) > 16 * 1024 * 1024) throw new Error('Release download failed');
  const chunks = [];
  let size = 0;
  for await (const chunk of response.body) {
    size += chunk.length;
    if (size > 16 * 1024 * 1024) throw new Error('Release download size limit exceeded');
    chunks.push(chunk);
  }
  return Buffer.concat(chunks);
}

export async function prepare({ target, archive, outputDir }) {
  const { lock, entry } = await readLock(target);
  const tgz = archive ? await readFile(archive) : await download(entry.tarballUrl);
  verifyIntegrity(tgz, entry.integrity);
  const bytes = extractBinary(tgz, entry.binaryPath);
  verifyBinary(bytes, entry, target);
  const directory = path.resolve(outputDir ?? path.join(projectRoot, 'apps/desktop/src-tauri/binaries'));
  await mkdir(directory, { recursive: true });
  const destination = path.join(directory, entry.resourceName);
  const temporary = `${destination}.${process.pid}.tmp`;
  try {
    await writeFile(temporary, bytes, { flag: 'wx' });
    if (target === 'aarch64-apple-darwin') await chmod(temporary, 0o755);
    await rename(temporary, destination);
  } finally { await rm(temporary, { force: true }); }
  return { version: lock.version, target, path: destination, bytes: bytes.length, sha256: entry.binarySha256 };
}

export function parseArguments(args, allowed) {
  const result = {};
  for (let i = 0; i < args.length; i += 2) {
    const key = args[i];
    if (!allowed.includes(key) || !args[i + 1] || args[i + 1].startsWith('--') || Object.hasOwn(result, key)) throw new Error('Invalid command arguments');
    result[key] = args[i + 1];
  }
  return result;
}

if (process.argv[1] && path.resolve(process.argv[1]) === fileURLToPath(import.meta.url)) {
  try {
    const args = parseArguments(process.argv.slice(2), ['--target', '--archive', '--output-dir']);
    console.log(JSON.stringify(await prepare({ target: args['--target'], archive: args['--archive'], outputDir: args['--output-dir'] })));
  } catch (error) { console.error(error.message); process.exitCode = 1; }
}
