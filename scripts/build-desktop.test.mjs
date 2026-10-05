import test from 'node:test';
import assert from 'node:assert/strict';
import { buildPlan } from './build-desktop.mjs';
import { readFileSync } from 'node:fs';
test('Windows full build prepares verified resources before NSIS', () => {
  const { steps } = buildPlan([], { platform: 'win32', arch: 'x64' });
  assert.ok(steps.find(s => s.includes('scripts/verify-sidecar.mjs')));
  assert.equal(steps.at(-1)[steps.at(-1).indexOf('--bundles') + 1], 'nsis');
  assert.deepEqual(steps.at(-1).slice(-2), ['--', '--locked']);
});
test('ARM64 full build chooses DMG; local host build omits the sidecar', () => {
  const mac = buildPlan([], { platform: 'darwin', arch: 'arm64' }).steps.at(-1);
  assert.equal(mac[mac.indexOf('--bundles') + 1], 'dmg');
  const local = buildPlan(['--local'], { platform: 'win32', arch: 'x64' });
  assert.ok(!local.steps.some(s => s.includes('scripts/prepare-ccusage.mjs')));
  assert.ok(local.steps.at(-1).includes('--no-bundle'));
});
test('unsupported architectures, shell-like targets and ambiguous options fail before execution', () => {
  assert.throws(() => buildPlan([], { platform: 'darwin', arch: 'x64' }));
  assert.throws(() => buildPlan(['--target=aarch64-apple-darwin'], { platform: 'win32', arch: 'x64' }));
  assert.throws(() => buildPlan(['--target=x64&echo'], { platform: 'win32', arch: 'x64' }));
  assert.throws(() => buildPlan(['--target', '', '--target=x64'], { platform: 'win32', arch: 'x64' }));
});
test('macOS packaging preserves the pinned upstream sidecar and checks its signature', () => {
  const config = JSON.parse(readFileSync(new URL('../apps/desktop/src-tauri/tauri.macos.conf.json', import.meta.url)));
  assert.deepEqual(config.bundle.externalBin, []);
  assert.equal(config.bundle.macOS.files['MacOS/ccusage'], 'binaries/ccusage-aarch64-apple-darwin');
  assert.ok(buildPlan([], { platform: 'darwin', arch: 'arm64' }).steps.some(step => step[0] === 'codesign' && step.includes('--verify')));
});
