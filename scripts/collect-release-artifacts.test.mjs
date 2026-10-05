import test from 'node:test';
import assert from 'node:assert/strict';
import { verifyEvidence } from './collect-release-artifacts.mjs';

test('collection accepts only native installed evidence for the exact version and commit', () => {
  const commit = 'a'.repeat(40);
  const target = 'x86_64-pc-windows-msvc';
  const entry = { binarySha256: 'b'.repeat(64), binaryBytes: 100 };
  const bundle = { appCommit: commit, target, version: '0.0.1', kind: 'installed', architectureVerified: true, runtime: { target, runtimeVerified: true }, installationSmoke: 'installed-sidecar-fixture', sidecar: { sha256: entry.binarySha256, bytes: 100 } };
  const build = { appCommit: commit, target };
  const expected = { target, version: '0.0.1', commit, entry };
  verifyEvidence(bundle, build, expected);
  for (const change of [{ appCommit: 'c'.repeat(40) }, { version: '0.0.2' }, { target: 'aarch64-apple-darwin' }, { kind: 'staging' }, { architectureVerified: false }, { runtime: { target, runtimeVerified: false } }, { sidecar: { sha256: 'c'.repeat(64), bytes: 100 } }]) {
    assert.throws(() => verifyEvidence({ ...bundle, ...change }, build, expected));
  }
  assert.throws(() => verifyEvidence(bundle, { ...build, appCommit: 'c'.repeat(40) }, expected));
});
