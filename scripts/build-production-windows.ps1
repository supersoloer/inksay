$ErrorActionPreference = 'Stop'
Set-Location (Join-Path $PSScriptRoot '..')
& node scripts/check-release.mjs
if ($LASTEXITCODE -ne 0) { throw 'Release preflight failed' }
if (!$env:INKSAY_BUILD_NUMBER) { $env:INKSAY_BUILD_NUMBER = [DateTimeOffset]::UtcNow.ToUnixTimeSeconds().ToString() }
if ($env:INKSAY_BUILD_NUMBER -notmatch '^\d+$') { throw 'INKSAY_BUILD_NUMBER must be numeric' }
& npm.cmd run tauri -- build --no-bundle --config src-tauri/tauri.production.conf.json --config src-tauri/tauri.windows.conf.json
if ($LASTEXITCODE -ne 0) { throw 'Windows build failed' }
$release = 'src-tauri/target/release'
$stage = Join-Path '.' "build/releases/windows/staging-$env:INKSAY_BUILD_NUMBER/Inksay"
if (Test-Path $stage) { throw "Staging directory already exists: $stage" }
New-Item -ItemType Directory -Path $stage -Force | Out-Null
Copy-Item "$release/inksay-tauri.exe" $stage
Copy-Item "$release/whisper-server.exe" $stage
Get-ChildItem "$release/*.dll" | Copy-Item -Destination $stage
Copy-Item "$release/resources" $stage -Recurse
& node scripts/package-release.mjs windows $stage
if ($LASTEXITCODE -ne 0) { throw 'Windows ZIP creation failed' }
