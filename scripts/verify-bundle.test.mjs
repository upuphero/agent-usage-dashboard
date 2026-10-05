import test from 'node:test';
import assert from 'node:assert/strict';
import { verifyMainArchitecture } from './verify-bundle.mjs';
test('PE verifier rejects truncated headers and non-x64 executable formats', () => {
  const pe = Buffer.alloc(128); pe.write('MZ'); pe.writeUInt32LE(64, 0x3c); pe.write('PE\0\0', 64); pe.writeUInt16LE(0x8664, 68); pe.writeUInt16LE(0x20b, 88);
  verifyMainArchitecture(pe, 'x86_64-pc-windows-msvc');
  pe.writeUInt32LE(10000, 0x3c); assert.throws(() => verifyMainArchitecture(pe, 'x86_64-pc-windows-msvc'));
  pe.writeUInt32LE(64, 0x3c); pe.writeUInt16LE(0x14c, 68); assert.throws(() => verifyMainArchitecture(pe, 'x86_64-pc-windows-msvc'));
});
test('Mach-O verifier accepts only thin ARM64, never Intel/universal', () => {
  const mach = Buffer.alloc(32); mach.writeUInt32LE(0xfeedfacf); mach.writeUInt32LE(0x0100000c, 4);
  verifyMainArchitecture(mach, 'aarch64-apple-darwin');
  mach.writeUInt32LE(0x01000007, 4); assert.throws(() => verifyMainArchitecture(mach, 'aarch64-apple-darwin'));
  mach.writeUInt32BE(0xcafebabe); assert.throws(() => verifyMainArchitecture(mach, 'aarch64-apple-darwin'));
});
