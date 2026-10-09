// A deliberately restricted generator for the contracts crate's dto!/enumeration! schema DSL.
// Unknown syntax/types fail closed. Rust serde structs and TS are emitted from the same declarations.
import { readFile, writeFile, mkdir } from 'node:fs/promises';
import { resolve } from 'node:path';
import { pathToFileURL } from 'node:url';
const root = resolve(import.meta.dirname, '..');
const camel = value => value.replace(/_([a-z])/g, (_, c) => c.toUpperCase());
const words = value => value.replace(/([a-z0-9])([A-Z])/g, '$1_$2');
export function generate(source) {
  const dtoMacro = source.slice(source.indexOf('macro_rules! dto'), source.indexOf('macro_rules! enumeration'));
  if (!dtoMacro.includes('rename_all = "camelCase", deny_unknown_fields')) throw new Error('Unsupported serde schema policy');
  const enums = [...source.matchAll(/enumeration!\(\s*(\w+)\s*,\s*"([\w-]+)"\s*,([\s\S]*?)\);/g)];
  const structs = [...source.matchAll(/dto!\(\s*(\w+)\s*\{([^}]+)\}\s*\);/g)];
  const generic = [...source.matchAll(/pub struct (\w+)<(\w+)>\s*\{([^}]+)\}/g)];
  if (enums.length !== (source.match(/\benumeration!\(/g) ?? []).length || structs.length !== (source.match(/\bdto!\(/g) ?? []).length) throw new Error('Unsupported schema macro syntax');
  if (!enums.length || !structs.length || generic.length !== 1) throw new Error('Unsupported contract declarations');
  const known = new Set([...enums, ...structs, ...generic].map(m => m[1]));
  const convert = (type, parameter) => {
    type = type.trim();
    if (type === 'String') return 'string'; if (type === 'bool') return 'boolean'; if (type === 'u32') return 'number';
    if (type === parameter) return parameter;
    const nested = /^(Option|Vec|Metric)<(.+)>$/.exec(type);
    if (nested) { const inner = convert(nested[2], parameter); return nested[1] === 'Option' ? `${inner} | null` : nested[1] === 'Vec' ? `Array<${inner}>` : `Metric<${inner}>`; }
    if (known.has(type)) return type;
    throw new Error(`Unsupported Rust contract type: ${type}`);
  };
  const fields = (body, parameter) => body.trim().replace(/,\s*$/, '').split(',').map(field => {
    const match = /^\s*(?:pub\s+)?(\w+)\s*:\s*([\w<>]+)\s*$/.exec(field);
    if (!match) throw new Error(`Unsupported field syntax: ${field}`);
    return `  ${camel(match[1])}: ${convert(match[2], parameter)};`;
  }).join('\n');
  let output = '// GENERATED from crates/usage-contracts/src/lib.rs. Do not edit.\n';
  for (const name of ['API_VERSION', 'SCAN_EVENT', 'AUTO_COLLECTION_EVENT', 'TIMEZONE_EVENT']) {
    const match = new RegExp(`pub const ${name}: &str = ("[^"\\n]+")`).exec(source);
    if (!match) throw new Error(`Missing ${name}`); output += `export const ${name} = ${match[1]} as const;\n`;
  }
  const commands = /pub const COMMANDS:[^=]+=\s*&?\s*\[([\s\S]*?)\];/.exec(source);
  if (!commands) throw new Error('Missing command declarations');
  if (commands[1].replace(/\("(\w+)",\s*"(\w+)"\)/g, '').replace(/[\s,]/g, '')) throw new Error('Unsupported command declaration');
  output += 'export const COMMANDS = {\n';
  for (const match of commands[1].matchAll(/\("(\w+)",\s*"(\w+)"\)/g)) output += `  ${match[1]}: "${match[2]}",\n`;
  output += '} as const;\n';
  for (const [, name, rename, body] of enums) {
    if (!['kebab-case', 'SCREAMING_SNAKE_CASE'].includes(rename)) throw new Error(`Unsupported enum rename: ${rename}`);
    const variants = body.split(',').map(s => s.trim()).filter(Boolean);
    if (variants.some(s => !/^\w+$/.test(s))) throw new Error(`Unsupported enum: ${name}`);
    const values = variants.map(v => JSON.stringify(rename === 'kebab-case' ? words(v).toLowerCase().replaceAll('_', '-') : words(v).toUpperCase()));
    output += `export type ${name} = ${values.join(' | ')};\n`;
    if (name === 'ErrorCode') output += `export const ERROR_CODES = [${values.join(', ')}] as const;\n`;
  }
  for (const [, name, parameter, body] of generic) output += `export interface ${name}<${parameter}> {\n${fields(body, parameter)}\n}\n`;
  for (const [, name, body] of structs) output += `export interface ${name} {\n${fields(body)}\n}\n`;
  // Adding a plain struct/enum without extending the generator cannot silently omit a DTO.
  const plain = [...source.matchAll(/pub (?:struct|enum) (\w+)/g)].map(m => m[1]).filter(name => !['Metric'].includes(name));
  if (plain.some(name => name !== '$name')) throw new Error(`Use schema macros for DTOs: ${plain.join(', ')}`);
  return output;
}
if (process.argv[1] && import.meta.url === pathToFileURL(resolve(process.argv[1])).href) {
  const output = generate(await readFile(resolve(root, 'crates/usage-contracts/src/lib.rs'), 'utf8'));
  const path = resolve(root, 'apps/dashboard/src/api/generated/usage.ts');
  if (process.argv.includes('--check')) {
    if (await readFile(path, 'utf8') !== output) throw new Error('Contract drift; run pnpm contracts:generate');
    console.log('Generated contract drift check passed.');
  } else { await mkdir(resolve(path, '..'), { recursive: true }); await writeFile(path, output); console.log('Generated usage.ts from usage-contracts.'); }
}
