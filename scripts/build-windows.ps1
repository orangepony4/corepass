$ErrorActionPreference='Stop'
$projectRoot=Split-Path -Parent $PSScriptRoot
Set-Location -LiteralPath $projectRoot
& npm.cmd ci --no-audit --no-fund
if ($LASTEXITCODE) {throw 'npm ci failed'}
& npm.cmd run tauri build -- --no-bundle
if ($LASTEXITCODE) {throw 'Tauri build failed'}
$release=Join-Path $projectRoot 'artifacts\Corepass-0.3.0-windows-x64'
New-Item -ItemType Directory -Force -Path $release | Out-Null
Copy-Item -LiteralPath 'target\release\corepass.exe' -Destination (Join-Path $release 'Corepass-Next.exe')
Copy-Item -LiteralPath 'src-tauri\resources\engine' -Destination $release -Recurse -Force
Copy-Item -LiteralPath 'README.md','LICENSE','THIRD-PARTY.md' -Destination $release
Write-Output $release
