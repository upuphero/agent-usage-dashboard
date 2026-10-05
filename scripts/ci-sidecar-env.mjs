import { appendFile } from 'node:fs/promises';
import { resolve } from 'node:path';
import { readLock, projectRoot } from './prepare-ccusage.mjs';
const target = process.argv[2];
const { entry } = await readLock(target);
if (!process.env.GITHUB_ENV) throw new Error('GITHUB_ENV is required; local tests should set CCUSAGE_TEST_BINARY explicitly');
const binary = resolve(projectRoot, 'apps/desktop/src-tauri/binaries', entry.resourceName);
await appendFile(process.env.GITHUB_ENV, `CCUSAGE_TEST_BINARY=${binary}\n`);
