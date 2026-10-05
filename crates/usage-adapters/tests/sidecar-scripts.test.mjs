import test from 'node:test';
import assert from 'node:assert/strict';
import { createHash } from 'node:crypto';
import { gzipSync } from 'node:zlib';
import { extractBinary, verifyIntegrity, verifyBinary, parseArguments, readLock } from '../../../scripts/prepare-ccusage.mjs';

function archive(name, type = 48, data = Buffer.from('hello')) {
  const h = Buffer.alloc(512);
  h.write(name); h.write('0000755\0', 100); h.write('0000000\0', 108); h.write('0000000\0', 116); h.write(data.length.toString(8).padStart(11, '0') + '\0', 124); h.write('00000000000\0', 136); h.fill(32, 148, 156); h[156] = type; h.write('ustar\0', 257);
  const sum = h.reduce((a,b) => a+b, 0); h.write(sum.toString(8).padStart(6, '0') + '\0 ', 148);
  return gzipSync(Buffer.concat([h, data, Buffer.alloc((512 - data.length % 512) % 512), Buffer.alloc(1024)]));
}
test('only regular locked entries can be extracted; traversal, links and truncation are rejected', () => {
  assert.equal(extractBinary(archive('package/bin/ccusage'), 'package/bin/ccusage').toString(), 'hello');
  for (const name of ['../../evil', '/absolute', 'C:/absolute', 'package\\evil']) assert.throws(() => extractBinary(archive(name), name), /Unsafe/);
  for (const type of [49,50,120]) assert.throws(() => extractBinary(archive('package/bin/ccusage', type), 'package/bin/ccusage'), /unsupported/);
  assert.throws(() => extractBinary(archive('package/other'), 'package/bin/ccusage'), /absent/);
  assert.throws(() => extractBinary(Buffer.from('broken'), 'package/bin/ccusage'));
});
test('locked integrity and binary SHA cannot be replaced by download-time values', () => {
  const bytes = Buffer.from('test');
  const integrity = 'sha512-' + createHash('sha512').update(bytes).digest('base64');
  verifyIntegrity(bytes, integrity);
  assert.throws(() => verifyIntegrity(Buffer.from('changed'), integrity), /mismatch/);
  assert.throws(() => verifyBinary(bytes, { binarySha256: '0'.repeat(64) }, 'x86_64-pc-windows-msvc'), /mismatch/);
});
test('lock is limited to two target triples; malformed CLI options are rejected', async () => {
  await assert.rejects(readLock('x86_64-apple-darwin'), /Unsupported/);
  await assert.rejects(readLock('__proto__'), /Unsupported/);
  assert.equal((await readLock('aarch64-apple-darwin')).lock.version, '20.0.26');
  assert.throws(() => parseArguments(['--target'], ['--target']));
  assert.throws(() => parseArguments(['--target','a','--target','b'], ['--target']));
  assert.throws(() => parseArguments(['--execute','anything'], ['--target']));
});
