import test from 'node:test';
import assert from 'node:assert/strict';
import { readFile } from 'node:fs/promises';
const source = await readFile(new URL('../crates/usage-contracts/src/lib.rs', import.meta.url), 'utf8');
const commands = await readFile(new URL('../apps/desktop/src-tauri/src/commands.rs', import.meta.url), 'utf8');
const host = await readFile(new URL('../apps/desktop/src-tauri/src/lib.rs', import.meta.url), 'utf8');
test('every contract command is registered exactly once by the thin host', () => {
  const declared = [...source.matchAll(/\("\w+",\s*"(\w+)"\)/g)].map(m => m[1]).sort();
  const registered = [...host.matchAll(/commands::(\w+)/g)].map(m => m[1]).sort();
  const implemented = [...commands.matchAll(/#\[tauri::command\]\s*pub (?:async )?fn (\w+)/g)].map(m => m[1]).sort();
  assert.deepEqual(registered, declared);
  assert.deepEqual(implemented, declared);
});
