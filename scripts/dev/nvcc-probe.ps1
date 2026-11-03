# Dev helper: compile a tiny .cu file inside the TRELLIS Studio build environment (MSVC + CUDA + torch headers).
# Usage: nvcc-probe.ps1 -Source file.cu [-Extra "-allow-unsupported-compiler"] [-Vs 2019|2022|18]
param([Parameter(Mandatory = $true)][string]$Source, [string]$Permissive = "-Xcompiler=/permissive-", [string]$Extra = "", [string]$VsYear = "2019")
$Root = "$env:LOCALAPPDATA\TrellisStudio\runtime"
$py = "$Root\venv\Scripts\python.exe"
$cuda = "$Root\cuda\v12.4"
$vc = Get-ChildItem "C:\Program Files*\Microsoft Visual Studio\*\*\VC\Auxiliary\Build\vcvars64.bat" | Where-Object { $_.FullName -match $VsYear } | Select-Object -First 1
if (-not $vc) { throw "no vcvars for $VsYear" }
cmd /c "`"$($vc.FullName)`" >nul 2>&1 && set" | ForEach-Object { if ($_ -match '^([^=]+)=(.*)$') { [Environment]::SetEnvironmentVariable($Matches[1], $Matches[2], "Process") } }
$env:PATH = "$cuda\bin;$env:PATH"; $env:CUDA_HOME = $cuda
$inc = (& $py -c "from torch.utils.cpp_extension import include_paths;print(';'.join(include_paths()))") -split ';' | ForEach-Object { "-I`"$_`"" }
$pyinc = (& $py -c "import sysconfig;print(sysconfig.get_paths()['include'])")
$cmd = "nvcc -c `"$Source`" -o NUL -std=c++17 --expt-relaxed-constexpr --extended-lambda -Xcompiler=/std:c++17 -Xcompiler=/EHsc $Permissive -Xcompiler=/Zc:__cplusplus -DTORCH_EXTENSION_NAME=t -DTORCH_API_INCLUDE_EXTENSION_H -I`"$pyinc`" $($inc -join ' ') $Extra"
Write-Host "cl: $((cl 2>&1 | Select-Object -First 1))"
$out = Invoke-Expression "$cmd -diag-suppress 1388,1394,1379" 2>&1 | ForEach-Object { ([string]$_).Trim() } | Where-Object { $_ -match "error|ambiguous|could be|or +'|fatal" }
if ($out) { $out | Select-Object -First 8 | ForEach-Object { $_.Substring(0, [Math]::Min(200, $_.Length)) } } else { "COMPILES OK" }

