import test from 'node:test';
import assert from 'node:assert/strict';
import { readFile } from 'node:fs/promises';
import { generate } from './generate-contracts.mjs';
const source = await readFile(new URL('../crates/usage-contracts/src/lib.rs', import.meta.url), 'utf8');
test('generator retains nullable metrics, camelCase and stable code strings', () => {
  const generated = generate(source);
  assert.match(generated, /value: T \| null;/);
  assert.match(generated, /startScan: "start_scan"/);
  assert.match(generated, /"SCHEMA_UNSUPPORTED"/);
  assert.match(generated, /total: Metric<string>/);
  assert.match(generated, /lastSuccessAt: string \| null/);
});
test('generator fails on unsupported numeric and declaration additions', () => {
  assert.throws(() => generate(source.replace('total: Metric<String>', 'total: Metric<u64>')), /Unsupported Rust contract type/);
  assert.throws(() => generate(source + '\npub struct NewDto { pub x: String }'), /Use schema macros/);
  assert.throws(() => generate(source.replace('rename_all = "camelCase"', 'rename_all = "snake_case"')), /Unsupported serde schema policy/);
});
