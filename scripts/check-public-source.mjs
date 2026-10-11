import { execFileSync } from 'node:child_process';
import { readFileSync, existsSync } from 'node:fs';
import { resolve, dirname, relative, basename } from 'node:path';

const root = resolve(import.meta.dirname, '..');
const files = execFileSync('git', ['ls-files', '-z', '--cached', '--others', '--exclude-standard'], { cwd: root, encoding: 'utf8' }).split('\0').filter(Boolean);
const required = ['LICENSE', 'NOTICE', 'README.md', 'CONTRIBUTING.md', 'SECURITY.md', 'THIRD_PARTY_NOTICES.md', 'src-tauri/resources/THIRD-PARTY-LICENSES.txt'];
const errors = [];
for (const file of required) if (!existsSync(resolve(root, file))) errors.push(`Missing: ${file}`);
const attributes = execFileSync('git', ['check-attr', '-z', '--stdin', 'export-ignore'], { cwd: root, input: files.join('\0') + '\0', encoding: 'utf8' }).split('\0');
const excluded = new Set();
for (let i = 0; i + 2 < attributes.length; i += 3) if (attributes[i + 2] === 'set') excluded.add(attributes[i]);
const secretPatterns = [
  /\bsk-[A-Za-z0-9_-]{24,}\b/,
  /\b(?:ghp|gho|ghs|ghr)_[A-Za-z0-9]{30,}\b/,
  /-----BEGIN (?:RSA |EC |OPENSSH )?PRIVATE KEY-----/,
];
for (const file of new Set(files)) {
  if (/^(?:build|node_modules|dist|src-tauri\/(?:target|binaries|\.whisper-src)|ios)\//.test(file)
      || /(?:^|\/)\.env(?:\.|$)/.test(file) && !file.endsWith('.env.example')
      || /\.(?:p12|pfx|mobileprovision|exe|dll|dylib|flac|mp3|aiff)$/i.test(file)
      || /\.wav$/i.test(file) && file !== 'src-tauri/resources/asr-test.wav'
      || ['config.json', 'history.json', 'dictionary.json', 'term-memory.json', 'statistics.json', 'replacement-rules.json'].includes(basename(file))) errors.push(`Private/generated file tracked: ${file}`);
  if (!existsSync(resolve(root, file))) { errors.push(`Missing tracked file: ${file}`); continue; }
  const bytes = readFileSync(resolve(root, file));
  if (bytes.includes(0)) continue;
  const text = bytes.toString('utf8');
  if (secretPatterns.some(pattern => pattern.test(text))) errors.push(`Potential secret in: ${file}`);
  if (file.endsWith('.md') && !excluded.has(file)) {
    for (const match of text.matchAll(/\]\(([^\s)]+)\)/g)) {
      const target = match[1].split('#')[0];
      if (!target || /^(?:[a-z]+:|\/\/)/i.test(target)) continue;
      if (!existsSync(resolve(root, dirname(file), decodeURIComponent(target))) || excluded.has(relative(root, resolve(root, dirname(file), decodeURIComponent(target))))) errors.push(`Broken local link in ${file}: ${target}`);
    }
  }
}
if (JSON.parse(readFileSync(resolve(root, 'package.json'))).license !== 'GPL-3.0-only') errors.push('npm project license mismatch');
if (!/^license = "GPL-3\.0-only"$/m.test(readFileSync(resolve(root, 'src-tauri/Cargo.toml'), 'utf8'))) errors.push('Cargo project license mismatch');
for (const [source, bundled] of [['LICENSE', 'src-tauri/resources/INKSAY-LICENSE'], ['NOTICE', 'src-tauri/resources/INKSAY-NOTICE']]) {
  if (!existsSync(resolve(root, bundled)) || !readFileSync(resolve(root, source)).equals(readFileSync(resolve(root, bundled)))) errors.push(`Bundled notice differs from ${source}`);
}
if (errors.length) throw new Error(errors.join('\n'));
console.log(`Public-source checks passed (${new Set(files).size} files; secrets are checked without printing matches).`);
console.log('This scan does not replace history, screenshot, attribution, or signing review.');
