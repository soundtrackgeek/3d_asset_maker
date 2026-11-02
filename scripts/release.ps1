# Publish a new version: bump -> commit -> tag -> push. GitHub Actions then builds the installer and
# publishes the Release (.github/workflows/release.yml). The in-app updater reads that Release.
#
# Usage:  powershell -File scripts/release.ps1 -Version 0.2.1          # CI builds + publishes
#         powershell -File scripts/release.ps1 -Version 0.2.1 -Local   # build + publish from this PC instead
# Prereqs: CHANGELOG.md already has a "## [<Version>]" section; `gh auth status` is logged in.
param([Parameter(Mandatory = $true)][string]$Version, [switch]$Local)
$ErrorActionPreference = "Stop"
Set-Location (Split-Path $PSScriptRoot -Parent)

if ($Version -notmatch '^\d+\.\d+\.\d+$') { throw "Version must be MAJOR.MINOR.PATCH" }
$notes = & "$PSScriptRoot\release-notes.ps1" -Version $Version   # throws if CHANGELOG has no entry

# 1. bump workspace version
$cargo = Get-Content Cargo.toml -Raw
$cargo = [regex]::Replace($cargo, '(?m)^version = "[^"]+"', "version = `"$Version`"", 1)
Set-Content Cargo.toml $cargo -NoNewline
# keep Cargo.lock in step with the new version (cargo logs to stderr, which "Stop" would treat as fatal)
$ErrorActionPreference = "Continue"
cargo update --workspace
$ErrorActionPreference = "Stop"
if ($LASTEXITCODE -ne 0) { throw "cargo update failed" }

# 2. local build only when -Local (CI does it otherwise)
if ($Local) {
    & "$PSScriptRoot\build-dist.ps1"
}

# 3. commit, tag, push
git add -A
git commit -m "Release v$Version`n`nCo-Authored-By: Claude Sonnet 5.5 <noreply@anthropic.com>"
git tag -a "v$Version" -m "v$Version"
git push
git push origin "v$Version"

# 4. publish
if ($Local) {
    $notesFile = New-TemporaryFile
    Set-Content $notesFile $notes
    gh release create "v$Version" dist\TRELLIS-Studio-Setup.exe dist\t2.exe --title "v$Version" --notes-file $notesFile
    Remove-Item $notesFile
}
else {
    Write-Host "Waiting for the Release workflow..."
    Start-Sleep 10
    $id = gh run list --workflow release.yml --branch "v$Version" --limit 1 --json databaseId --jq '.[0].databaseId'
    if (-not $id) { throw "release workflow run not found; check the Actions tab" }
    gh run watch $id --exit-status
    if ($LASTEXITCODE -ne 0) { throw "release workflow failed (gh run view $id --log-failed)" }
}
gh release view "v$Version" --json assets --jq '.assets[].name'

# 5. AGENTS.md rule: cargo clean after pushing
cargo clean
Write-Host "Released v$Version"
