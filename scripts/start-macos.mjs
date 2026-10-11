import { execFileSync, spawn } from 'node:child_process';
import { mkdirSync, openSync, closeSync } from 'node:fs';
import { resolve } from 'node:path';
if (process.platform !== 'darwin') throw new Error('macOS launcher requires macOS');
const root = resolve(import.meta.dirname, '..');
const bundle = resolve(root, 'src-tauri/target/release/bundle/macos/InkSay.app');
const identifier = execFileSync('/usr/libexec/PlistBuddy', ['-c', 'Print CFBundleIdentifier', `${bundle}/Contents/Info.plist`], { encoding: 'utf8' }).trim();
if (identifier !== 'com.supersoloer.inksay.macos') throw new Error(`Unexpected app identity: ${identifier}`);
const binary = `${bundle}/Contents/MacOS/inksay-tauri`;
process.stdout.write(execFileSync(binary, ['--build-info'], { encoding: 'utf8' }));
mkdirSync(resolve(root, 'build'), { recursive: true });
const log = openSync(resolve(root, 'build/launch.log'), 'a', 0o600);
spawn(binary, [], { cwd: root, detached: true, stdio: ['ignore', log, log] }).unref();
// Show the main window through single-instance IPC, without same-name app resolution.
setTimeout(() => {
  spawn(binary, [], { cwd: root, detached: true, stdio: ['ignore', log, log] }).unref();
  closeSync(log);
}, 2000);
