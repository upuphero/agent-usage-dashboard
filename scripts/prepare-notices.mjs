import { readFile, writeFile } from 'node:fs/promises';
import { resolve } from 'node:path';
import { pathToFileURL } from 'node:url';
import { extractBinary, verifyIntegrity, projectRoot } from './prepare-ccusage.mjs';

export function notices(lock, archive) {
  verifyIntegrity(archive, lock.license.packageIntegrity);
  const license = extractBinary(archive, 'package/LICENSE').toString('utf8').replaceAll('\r\n', '\n');
  if (!license.includes('MIT License') || !license.includes('Copyright (c) 2025 ryoppippi')) throw new Error('Unexpected ccusage license; review upstream notices');
  return `# Third-party notices\n\nDevelopment inventory; additional transitive dependency notices must be reviewed before public distribution.\n\n## ccusage ${lock.version}\n\nSource commit: ${lock.sourceCommit}\nLicense source: ${lock.license.source}\nThe license below was extracted from the locked, SRI-verified main npm package. Native binaries are pinned separately in ccusage.lock.json.\n\n\`\`\`text\n${license.trim()}\n\`\`\`\n\n## Remaining inventory\n\nRust/npm dependency licenses and the complete native binary dependency inventory remain release gates. This document does not certify that the inventory is complete.\n`;
}
if (process.argv[1] && import.meta.url === pathToFileURL(resolve(process.argv[1])).href) {
  const lock = JSON.parse(await readFile(resolve(projectRoot, 'ccusage.lock.json'), 'utf8'));
  const index = process.argv.indexOf('--archive');
  const archivePath = index >= 0 ? process.argv[index + 1] : null;
  const url = `https://registry.npmjs.org/ccusage/-/ccusage-${lock.version}.tgz`;
  if (lock.license.source !== `${url}#package/LICENSE`) throw new Error('Unexpected locked license URL');
  let archive;
  if (archivePath) archive = await readFile(resolve(archivePath));
  else {
    const response = await fetch(url, { signal: AbortSignal.timeout(30_000) });
    if (!response.ok) throw new Error(`License archive download failed: ${response.status}`);
    const chunks = []; let size = 0;
    for await (const chunk of response.body) { size += chunk.length; if (size > 16 * 1024 * 1024) throw new Error('License archive exceeds limit'); chunks.push(chunk); }
    archive = Buffer.concat(chunks);
  }
  const expected = notices(lock, archive);
  const path = resolve(projectRoot, 'THIRD_PARTY_NOTICES.md');
  if (process.argv.includes('--check')) { if (await readFile(path, 'utf8') !== expected) throw new Error('Third-party notices drift; run node scripts/prepare-notices.mjs'); }
  else await writeFile(path, expected);
  console.log(`Verified ccusage ${lock.version} license notice; transitive inventory remains pending.`);
}
