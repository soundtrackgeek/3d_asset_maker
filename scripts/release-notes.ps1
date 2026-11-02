# Print the CHANGELOG.md section body for a version (used as GitHub Release notes). Fails if missing.
param([Parameter(Mandatory = $true)][string]$Version)
$ErrorActionPreference = "Stop"
$changelog = Get-Content (Join-Path (Split-Path $PSScriptRoot -Parent) "CHANGELOG.md") -Raw
$v = [regex]::Escape($Version)
$m = [regex]::Match($changelog, "(?ms)^## \[$v\][^\n]*\n(.*?)(?=^## \[|\z)")
if (-not $m.Success) { throw "CHANGELOG.md has no '## [$Version]' section" }
$m.Groups[1].Value.Trim()
