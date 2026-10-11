import { execFileSync } from 'node:child_process';
import { readFileSync, mkdirSync, existsSync, writeFileSync } from 'node:fs';
import { resolve } from 'node:path';
import { createHash } from 'node:crypto';

const root = resolve(import.meta.dirname, '..');
execFileSync(process.execPath, ['scripts/check-public-source.mjs'], { cwd: root, stdio: 'inherit' });
if (execFileSync('git', ['status', '--porcelain'], { cwd: root, encoding: 'utf8' }).trim()) throw new Error('Commit reviewed changes before exporting source.');
const commit = execFileSync('git', ['rev-parse', 'HEAD'], { cwd: root, encoding: 'utf8' }).trim();
const version = JSON.parse(readFileSync(resolve(root, 'package.json'))).version;
const directory = resolve(root, 'build/releases/source');
mkdirSync(directory, { recursive: true });
const archive = resolve(directory, `InkSay_V${version}_Source_${commit.slice(0, 8)}.tar.gz`);
if (existsSync(archive)) throw new Error('Source archive already exists.');
execFileSync('git', ['archive', '--format=tar.gz', '--prefix=InkSay/', `--output=${archive}`, 'HEAD'], { cwd: root });
writeFileSync(`${archive}.manifest.json`, JSON.stringify({ version, commit, license: 'GPL-3.0-only', sha256: createHash('sha256').update(readFileSync(archive)).digest('hex'), exclusions: '.gitattributes export-ignore; archives, personal data and build outputs are not tracked' }, null, 2) + '\n');
console.log(archive);
