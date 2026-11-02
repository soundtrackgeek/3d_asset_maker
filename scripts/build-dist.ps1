# Build release artifacts into dist/: TRELLIS-Studio-Setup.exe (NSIS installer) and t2.exe.
# Shared by CI (.github/workflows/release.yml) and `release.ps1 -Local`.
# Needs: Rust, and `cargo install tauri-cli --version "^2"`.
$ErrorActionPreference = "Stop"
Set-Location (Split-Path $PSScriptRoot -Parent)

# t2.exe first: the installer bundles it (see bundle.resources in src-tauri/tauri.conf.json)
cargo build --release -p trellis-studio-cli
if ($LASTEXITCODE -ne 0) { throw "cli build failed" }
cargo build --release
if ($LASTEXITCODE -ne 0) { throw "build failed" }
cargo tauri build --bundles nsis
if ($LASTEXITCODE -ne 0) { throw "installer build failed" }

New-Item -ItemType Directory -Force dist | Out-Null
$setup = Get-ChildItem target\release\bundle\nsis\*-setup.exe | Sort-Object LastWriteTime -Descending | Select-Object -First 1
Copy-Item $setup.FullName dist\TRELLIS-Studio-Setup.exe -Force
Copy-Item target\release\t2.exe dist\t2.exe -Force
Get-ChildItem dist | Select-Object Name, Length
