# Publish a new version: bump -> build -> commit -> tag -> push -> GitHub Release with the .exe files -> cargo clean.
# Usage:  powershell -File scripts/release.ps1 -Version 0.2.0
# Prereqs: CHANGELOG.md already has a "## [<Version>]" section; `gh auth status` is logged in.
param([Parameter(Mandatory = $true)][string]$Version)
$ErrorActionPreference = "Stop"
Set-Location (Split-Path $PSScriptRoot -Parent)

if ($Version -notmatch '^\d+\.\d+\.\d+$') { throw "Version must be MAJOR.MINOR.PATCH" }
$changelog = Get-Content CHANGELOG.md -Raw
if ($changelog -notmatch "(?m)^## \[$([regex]::Escape($Version))\]") { throw "CHANGELOG.md has no '## [$Version]' section" }
# Release notes = the body of this version's section.
$m = [regex]::Match($changelog, "(?ms)^## \[$([regex]::Escape($Version))\][^\n]*\n(.*?)(?=^## \[|\z)")
$notes = $m.Groups[1].Value.Trim()

# 1. bump workspace version
$cargo = Get-Content Cargo.toml -Raw
$cargo = [regex]::Replace($cargo, '(?m)^version = "[^"]+"', "version = `"$Version`"", 1)
Set-Content Cargo.toml $cargo -NoNewline

# 2. build
cargo build --release
if ($LASTEXITCODE -ne 0) { throw "build failed" }

# 3. collect exe files
New-Item -ItemType Directory -Force dist | Out-Null
Copy-Item target\release\trellis-studio.exe dist\TRELLIS-Studio.exe -Force
Copy-Item target\release\t2.exe dist\t2.exe -Force

# 4. commit, tag, push
git add -A
git commit -m "Release v$Version`n`nCo-Authored-By: Claude Sonnet 5.5 <noreply@anthropic.com>"
git tag -a "v$Version" -m "v$Version"
git push
git push origin "v$Version"

# 5. publish the exe files as GitHub Release assets (keeps the git history free of binaries)
$notesFile = New-TemporaryFile
Set-Content $notesFile $notes
gh release create "v$Version" dist\TRELLIS-Studio.exe dist\t2.exe --title "v$Version" --notes-file $notesFile
Remove-Item $notesFile

# 6. AGENTS.md rule: cargo clean after pushing
cargo clean
Write-Host "Released v$Version"
