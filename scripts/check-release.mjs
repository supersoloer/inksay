import { readFileSync } from 'node:fs';
import { execFileSync } from 'node:child_process';
const version = JSON.parse(readFileSync('package.json')).version;
const config = JSON.parse(readFileSync('src-tauri/tauri.conf.json')).version;
const cargo = readFileSync('src-tauri/Cargo.toml', 'utf8').match(/^version = "([^"]+)"/m)?.[1];
const npmLock = JSON.parse(readFileSync('package-lock.json'));
const rustLock = readFileSync('src-tauri/Cargo.lock', 'utf8').match(/name = "inksay-tauri"\nversion = "([^"]+)"/)?.[1];
if (version !== config || version !== cargo || version !== npmLock.version || version !== npmLock.packages[''].version || version !== rustLock) throw new Error('Version mismatch between npm, Cargo and Tauri');
if (execFileSync('git', ['status', '--porcelain'], { encoding: 'utf8' }).trim()) throw new Error('Commit source changes before creating a release');
