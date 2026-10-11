import { readFileSync, writeFileSync } from 'node:fs';
const version = process.argv[2];
if (!/^\d+\.\d+\.\d+$/.test(version ?? '')) throw new Error('Usage: npm run version:set -- 0.1.1');
for (const name of ['package.json', 'package-lock.json', 'src-tauri/tauri.conf.json']) {
  const data = JSON.parse(readFileSync(name)); data.version = version;
  if (data.packages) data.packages[''].version = version;
  writeFileSync(name, JSON.stringify(data, null, 2) + '\n');
}
const cargo = 'src-tauri/Cargo.toml';
writeFileSync(cargo, readFileSync(cargo, 'utf8').replace(/^version = "[^"]+"/m, `version = "${version}"`));
const lock = 'src-tauri/Cargo.lock';
writeFileSync(lock, readFileSync(lock, 'utf8').replace(/(name = "inksay-tauri"\nversion = ")[^"]+/, `$1${version}`));
