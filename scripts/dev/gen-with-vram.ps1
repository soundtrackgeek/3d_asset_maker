# Dev helper: run `t2 generate` then print peak VRAM (reported by the worker) and any native crash trace.
# Usage: gen-with-vram.ps1 -Image samples\robot.png -Preset balanced
param([Parameter(Mandatory = $true)][string]$Image, [string]$Preset = "balanced", [string]$Out = "target\gen-out-$Preset")
$log = Join-Path $env:TEMP "t2-gen-$Preset.log"
& "$PSScriptRoot\..\..\target\release\t2.exe" generate $Image --preset $Preset --engine unity --out $Out *> $log
$code = $LASTEXITCODE
"exit $code"
$job = Get-ChildItem "$env:LOCALAPPDATA\TrellisStudio\jobs" | Sort-Object LastWriteTime | Select-Object -Last 1
"job $($job.Name)"
$j = Get-Content (Join-Path $job.FullName "job.json") -Raw | ConvertFrom-Json
"status: $($j.status), elapsed $([int]$j.elapsed_secs)s, peak VRAM $($j.peak_vram_mb) MiB"
if ($j.error) { $j.error.Substring(0, [Math]::Min(300, $j.error.Length)) }
Get-Content (Join-Path $job.FullName "worker.log") | Where-Object { $_ -match "Windows fatal|Current thread|^\s+File " -and $_ -notmatch "FutureWarning" } | Select-Object -Last 14 | ForEach-Object { $_.Substring(0, [Math]::Min(230, $_.Length)) }
