import { createHash } from "node:crypto";
import { readFileSync, writeFileSync, mkdirSync, existsSync } from 'node:fs';
import { resolve, basename, dirname, join } from 'node:path';
import { execFileSync } from 'node:child_process';
const [platform, sourceArg] = process.argv.slice(2);
const build = process.env.INKSAY_BUILD_NUMBER;
if (!['macos', 'windows'].includes(platform) || !sourceArg || !/^\d+$/.test(build ?? '')) {
  throw new Error('Usage: INKSAY_BUILD_NUMBER=<number> node scripts/package-release.mjs <macos|windows> <app-or-directory>');
}
const version = JSON.parse(readFileSync(new URL('../src-tauri/tauri.conf.json', import.meta.url))).version;
if (!/^\d+\.\d+\.\d+$/.test(version)) throw new Error('Release version must contain major.minor.patch');
const source = resolve(sourceArg);
if (!existsSync(source)) throw new Error('Release source does not exist');
const out = resolve(import.meta.dirname, '../build/releases', platform);
mkdirSync(out, { recursive: true });
const archive = join(out, `InkSay_V${version}.${build}.zip`);
if (existsSync(archive)) throw new Error(`Archive already exists: ${archive}`);
const binary = platform === 'macos' ? join(source, 'Contents/MacOS/inksay-tauri') : join(source, 'inksay-tauri.exe');
const metadata = JSON.parse(execFileSync(binary, ['--build-info'], { encoding: 'utf8' }));
if (metadata.build !== build || metadata.version !== version || metadata.dirty) throw new Error('Release metadata mismatch or dirty source tree');
if (process.platform === 'darwin') {
  execFileSync('ditto', ['-c', '-k', '--keepParent', ...(platform === 'windows' ? ['--norsrc', '--noextattr'] : []), source, archive], { stdio: 'inherit' });
} else if (process.platform === 'win32') {
  const quote = value => "'" + value.replaceAll("'", "''") + "'";
  execFileSync('powershell.exe', ['-NoProfile', '-Command', `Compress-Archive -LiteralPath ${quote(source)} -DestinationPath ${quote(archive)} -CompressionLevel Optimal`], { stdio: 'inherit' });
} else {
  execFileSync('zip', ['-qr', archive, basename(source)], { cwd: dirname(source), stdio: 'inherit' });
}
const sha256 = createHash('sha256').update(readFileSync(archive)).digest('hex');
writeFileSync(`${archive}.manifest.json`, JSON.stringify({ ...metadata, platform, archive: basename(archive), sha256 }, null, 2) + '\n');
console.log(archive);
