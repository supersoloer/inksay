import { existsSync, lstatSync, rmSync } from 'node:fs';
import { resolve } from 'node:path';

const root = resolve(import.meta.dirname, '..');
const dryRun = process.argv.slice(2).includes('--dry-run');
const paths = ['src-tauri/target/debug', 'ios/DerivedData', 'ios/DeviceDerivedData'];
for (const target of ['src-tauri/target/release', 'src-tauri/target/x86_64-pc-windows-gnu/debug', 'src-tauri/target/x86_64-pc-windows-gnu/release']) {
  for (const cache of ['deps', 'build', 'incremental', 'examples', '.fingerprint']) paths.push(`${target}/${cache}`);
}
for (const relative of paths) {
  const path = resolve(root, relative);
  if (!existsSync(path)) continue;
  if (lstatSync(path).isSymbolicLink()) throw new Error(`Refusing symlink: ${relative}`);
  console.log(`${dryRun ? 'Would remove' : 'Removing'} cache: ${relative}`);
  if (!dryRun) rmSync(path, { recursive: true });
}
