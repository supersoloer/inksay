import { execFileSync } from 'node:child_process';
import { readFileSync, readdirSync, existsSync, statSync, writeFileSync } from 'node:fs';
import { resolve, dirname, relative } from 'node:path';
import { createHash } from 'node:crypto';

const root = resolve(import.meta.dirname, '..');
const metadata = platform => JSON.parse(execFileSync('cargo', ['metadata', '--manifest-path', 'src-tauri/Cargo.toml', '--locked', '--format-version', '1', ...(platform ? ['--filter-platform', platform] : [])], { cwd: root, encoding: 'utf8', maxBuffer: 32 * 1024 * 1024 }));
const all = metadata();
const mac = metadata('aarch64-apple-darwin');
const win = metadata('x86_64-pc-windows-msvc');
const native = new Set([...mac.resolve.nodes, ...win.resolve.nodes].map(node => node.id));
const supplement = JSON.parse(readFileSync(resolve(root, 'docs/licenses/upstream-notices.json')));
const npmSupplement = JSON.parse(readFileSync(resolve(root, 'docs/licenses/npm-upstream-notices.json')));
const groups = new Map();
const inventory = [];
const filename = /^(?:licen[cs]e|copying|notice|copyright)(?:$|[-._])/i;
function add(component, records) {
  for (const record of records) {
    const hash = createHash('sha256').update(record.text).digest('hex');
    if (!groups.has(hash)) groups.set(hash, { text: record.text, sources: new Set(), components: new Set() });
    const group = groups.get(hash);
    group.components.add(component);
    group.sources.add(record.url);
  }
}
function licenseFiles(directory, recurse = false) {
  const result = [];
  for (const name of readdirSync(directory).sort()) {
    const path = resolve(directory, name);
    if (statSync(path).isDirectory()) {
      if (recurse && name !== 'node_modules') result.push(...licenseFiles(path, true));
    } else if (filename.test(name)) result.push(path);
  }
  return result;
}
for (const pkg of all.packages.filter(pkg => pkg.source).sort((a, b) => `${a.name}@${a.version}`.localeCompare(`${b.name}@${b.version}`, 'en'))) {
  const key = `${pkg.name}@${pkg.version}`;
  inventory.push({ ecosystem: 'cargo', name: pkg.name, version: pkg.version, license: pkg.license, repository: pkg.repository, noticeScope: native.has(pkg.id) ? 'native-targets' : 'other-lockfile-targets' });
  if (!native.has(pkg.id)) continue;
  const base = dirname(pkg.manifest_path);
  const files = licenseFiles(base);
  if (pkg.license_file && existsSync(resolve(base, pkg.license_file))) files.push(resolve(base, pkg.license_file));
  const records = files.map(path => ({ url: `https://crates.io/crates/${pkg.name}/${pkg.version}#${relative(base, path)}`, text: readFileSync(path, 'utf8') }));
  if (!records.length) records.push(...(supplement[key] ?? []));
  if (!records.length) throw new Error(`Missing upstream license text: ${key}`);
  add(`Cargo: ${key} (${pkg.license})`, records);
}
const lock = JSON.parse(readFileSync(resolve(root, 'package-lock.json')));
for (const [path, pkg] of Object.entries(lock.packages).filter(([path]) => path).sort(([a], [b]) => a.localeCompare(b, 'en'))) {
  const name = path.split('node_modules/').at(-1);
  inventory.push({ ecosystem: 'npm', name, version: pkg.version, license: pkg.license, development: Boolean(pkg.dev), optional: Boolean(pkg.optional) });
  const directory = resolve(root, path);
  if (!existsSync(directory)) {
    if (!pkg.dev && !pkg.optional) throw new Error(`Install npm dependency before collecting notices: ${name}`);
    continue;
  }
  const records = licenseFiles(directory, true).map(file => ({ url: `https://www.npmjs.com/package/${name}/v/${pkg.version}#${relative(directory, file)}`, text: readFileSync(file, 'utf8') }));
  records.push(...(npmSupplement[`${name}@${pkg.version}`] ?? []));
  if (!records.length && !pkg.dev && !pkg.optional) throw new Error(`Missing npm license text: ${name}@${pkg.version}`);
  add(`npm: ${name}@${pkg.version} (${pkg.license})`, records);
}
const notice = ['InkSay third-party license notices', 'Generated from locked Cargo/npm dependencies. Texts retain upstream terms.', 'Cargo text scope: macOS ARM64 and Windows x64/MSVC including build/test dependencies. npm text scope: packages installed for this generation.', 'The full lockfile inventory includes other platforms; it is not a list of binaries all shipped on one platform.', ''];
for (const group of groups.values()) notice.push('='.repeat(72), [...group.components].sort().join('\n'), 'Sources:', [...group.sources].sort().join('\n'), '', group.text.trim(), '');
writeFileSync(resolve(root, 'src-tauri/resources/THIRD-PARTY-LICENSES.txt'), notice.join('\n'));
writeFileSync(resolve(root, 'docs/licenses/dependencies.json'), JSON.stringify({ cargoTargets: ['aarch64-apple-darwin', 'x86_64-pc-windows-msvc'], npmNotices: 'installed packages; platform-specific optional packages vary', dependencies: inventory }, null, 2) + '\n');
console.log(`Collected ${inventory.length} dependency records and ${groups.size} distinct notice texts.`);
