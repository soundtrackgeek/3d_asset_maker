# TRELLIS.2 runtime installer for Windows + NVIDIA (mirrors upstream setup.sh: torch 2.6.0 / CUDA 12.4).
# Creates <Root>\venv, clones microsoft/TRELLIS.2, builds the native CUDA extensions
# (nvdiffrast, nvdiffrec, CuMesh, FlexGEMM, o-voxel) and verifies that everything imports.
#
# Build prerequisites (checked up front; nothing long runs if they are missing):
#   * Visual Studio Build Tools with the C++ workload (2019 or 2022 preferred)
#   * CUDA Toolkit 12.x (12.4 recommended). A system install is used if present; otherwise the
#     needed components are downloaded from NVIDIA into <Root>\cuda (set T2_SKIP_CUDA_INSTALL=1 to forbid that).
param(
    [Parameter(Mandatory = $true)][string]$Root,
    [string]$PythonVersion = "3.12",
    [string]$CudaVersion = "12.4",
    # Rebuild only these extensions (nvdiffrast, nvdiffrec, CuMesh, FlexGEMM, o-voxel); skips venv/torch/deps.
    [string[]]$Only = @()
)
$ErrorActionPreference = "Continue"
$Only = @($Only | ForEach-Object { $_ -split ',' } | Where-Object { $_ })   # -File passes "a,b" as one string
New-Item -ItemType Directory -Force $Root | Out-Null
Set-Location $Root
$logs = Join-Path $Root "logs"
New-Item -ItemType Directory -Force $logs | Out-Null
$venv = Join-Path $Root "venv"
$py = Join-Path $venv "Scripts\python.exe"
$repo = Join-Path $Root "TRELLIS.2"
$extDir = Join-Path $Root "extensions"
$failed = @()
$hasUv = [bool](Get-Command uv -ErrorAction SilentlyContinue)

function Step($m) { Write-Host ""; Write-Host "==> $m" }

# Run a native command, stream its output, keep a log, and record failure with the log tail.
function Exec([string]$label, [scriptblock]$cmd) {
    Step $label
    $log = Join-Path $logs (($label -replace '[^A-Za-z0-9]+', '_') + ".log")
    & $cmd 2>&1 | ForEach-Object { "$_" } | Tee-Object -FilePath $log | ForEach-Object { Write-Host $_ }
    if ($LASTEXITCODE -ne 0) {
        Write-Host "!! FAILED: $label (full log: $log)"
        Write-Host "---- last lines ----"
        Get-Content $log -Tail 25 | ForEach-Object { Write-Host $_ }
        $script:failed += $label
        return $false
    }
    return $true
}

function PipInstall {
    if ($hasUv) { uv pip install --python $py @args } else { & $py -m pip install @args }
}

# ---------------------------------------------------------------- 1. python venv
if (-not (Test-Path $py)) {
    if ($hasUv) { Exec "create venv (uv, Python $PythonVersion)" { uv venv --python $PythonVersion $venv } | Out-Null }
    elseif (Get-Command py -ErrorAction SilentlyContinue) { Exec "create venv (py $PythonVersion)" { py "-$PythonVersion" -m venv $venv } | Out-Null }
    else { Write-Host "!! Need 'uv' (winget install astral-sh.uv) or the 'py' launcher with Python $PythonVersion"; exit 1 }
}
if (-not (Test-Path $py)) { Write-Host "!! venv was not created"; exit 1 }

# ---------------------------------------------------------------- 2. TRELLIS.2 repo
if (-not (Test-Path (Join-Path $repo "o-voxel\setup.py"))) {
    if (Test-Path $repo) { Remove-Item -Recurse -Force $repo }
    Exec "clone TRELLIS.2" { git clone -b main --recursive https://github.com/microsoft/TRELLIS.2.git $repo } | Out-Null
}
else {
    git -C $repo submodule update --init --recursive 2>&1 | Out-Null
}

# ---------------------------------------------------------------- 3. torch + python deps
if ($Only.Count -eq 0) {
    $torchOk = (& $py -c "import torch;print(torch.__version__)" 2>$null)
    if ($torchOk -notlike "2.6.0*") {
        Exec "install PyTorch 2.6.0 (CUDA 12.4)" { PipInstall torch==2.6.0 torchvision==0.21.0 --index-url https://download.pytorch.org/whl/cu124 } | Out-Null
    }
    # TRELLIS.2 sparse attention supports only xformers / flash_attn (no sdpa). xformers has official Windows wheels;
    # it needs the Triton that matches torch 2.6 (3.2.x), not the newest triton-windows.
    Exec "install xformers (attention backend)" {
        PipInstall "xformers==0.0.29.post3" --index-url https://download.pytorch.org/whl/cu124 --extra-index-url https://pypi.org/simple
        PipInstall "triton-windows>=3.2,<3.3"
    } | Out-Null
    Exec "install Python dependencies" {
        PipInstall setuptools wheel ninja numpy plyfile pillow imageio imageio-ffmpeg tqdm easydict opencv-python-headless trimesh "transformers>=4.56,<5" tensorboard pandas lpips zstandard kornia timm huggingface_hub safetensors
        PipInstall "git+https://github.com/EasternJournalist/utils3d.git@9a4eb15e4021b67b12c460c7057d642626897ec8"
    } | Out-Null
}

# ---------------------------------------------------------------- 4. build environment preflight
Step "check build tools (CUDA Toolkit + C++ compiler)"

function Find-Cuda {
    $cands = @((Join-Path $Root "cuda\v$CudaVersion"))   # our own user-level copy first
    if ($env:CUDA_PATH) { $cands += $env:CUDA_PATH }
    $base = Join-Path $env:ProgramFiles "NVIDIA GPU Computing Toolkit\CUDA"
    $cands += (Join-Path $base "v$CudaVersion")
    if (Test-Path $base) { $cands += (Get-ChildItem $base -Directory | Sort-Object Name -Descending | ForEach-Object { $_.FullName }) }
    foreach ($c in $cands) {
        $nvcc = Join-Path $c "bin\nvcc.exe"
        if (Test-Path $nvcc) {
            $v = (& $nvcc --version 2>&1 | Out-String)
            if ($v -match "release 12\.") { return $c }   # torch cu12x wheels need a CUDA 12 toolkit
        }
    }
    return $null
}

# No system-wide CUDA install needed: NVIDIA publishes each toolkit component as a checksummed zip.
# We assemble just what building the extensions needs (nvcc, headers, dev libs; ~1.2 GB) into
# <Root>\cuda\v<version>. No admin rights, no driver or NVIDIA-app components touched.
function Install-CudaRedist([string]$target) {
    $ver = "$CudaVersion.1"
    $base = "https://developer.download.nvidia.com/compute/cuda/redist"
    $manifest = Invoke-RestMethod "$base/redistrib_$ver.json"
    $parts = "cuda_nvcc", "cuda_cudart", "cuda_cccl", "cuda_nvrtc", "cuda_profiler_api", "cuda_nvtx", "cuda_nvml_dev",
             "libcublas", "libcusparse", "libcusolver", "libcurand", "libcufft", "libnvjitlink"
    $dl = Join-Path $Root "downloads"
    New-Item -ItemType Directory -Force $dl, $target | Out-Null
    foreach ($p in $parts) {
        $w = $manifest.$p.'windows-x86_64'
        if (-not $w) { Write-Host "!! $p missing from NVIDIA manifest"; return $false }
        $zip = Join-Path $dl (Split-Path $w.relative_path -Leaf)
        Write-Host ("downloading {0} {1} ({2:N0} MB)" -f $p, $manifest.$p.version, ($w.size / 1MB))
        if (-not (Test-Path $zip) -or (Get-FileHash $zip -Algorithm SHA256).Hash -ne $w.sha256.ToUpper()) {
            curl.exe -sS -L --fail --retry 3 -o $zip "$base/$($w.relative_path)"
            if ($LASTEXITCODE -ne 0) { Write-Host "!! download failed: $p"; return $false }
        }
        if ((Get-FileHash $zip -Algorithm SHA256).Hash -ne $w.sha256.ToUpper()) { Write-Host "!! checksum mismatch: $p"; Remove-Item $zip; return $false }
        $tmp = Join-Path $dl "x_$p"
        if (Test-Path $tmp) { Remove-Item -Recurse -Force $tmp }
        New-Item -ItemType Directory $tmp | Out-Null
        tar -xf $zip -C $tmp
        if ($LASTEXITCODE -ne 0) { Write-Host "!! extract failed: $p"; return $false }
        $inner = Get-ChildItem $tmp -Directory | Select-Object -First 1
        Copy-Item -Path (Join-Path $inner.FullName "*") -Destination $target -Recurse -Force
        Remove-Item -Recurse -Force $tmp
        Remove-Item $zip
    }
    Remove-Item -Recurse -Force $dl -ErrorAction SilentlyContinue
    return (Test-Path (Join-Path $target "bin\nvcc.exe"))
}

$cuda = Find-Cuda
if (-not $cuda -and $env:T2_SKIP_CUDA_INSTALL -ne "1") {
    Write-Host "CUDA Toolkit $CudaVersion not found. Downloading the build components from NVIDIA (~1.2 GB, no admin needed)..."
    $target = Join-Path $Root "cuda\v$CudaVersion"
    if (Install-CudaRedist $target) { $cuda = Find-Cuda } else { Write-Host "!! CUDA component download failed" }
}
if (-not $cuda) {
    Write-Host "!! CUDA Toolkit 12.x (nvcc) not found. Re-run setup, or install CUDA $CudaVersion from https://developer.nvidia.com/cuda-12-4-0-download-archive"
    exit 4
}
Write-Host "CUDA Toolkit: $cuda"

# Pick a Visual Studio the CUDA 12 toolchain supports: 2022 (17) > 2019 (16) > anything else.
$vswhere = Join-Path ${env:ProgramFiles(x86)} "Microsoft Visual Studio\Installer\vswhere.exe"
$vcvars = $null; $vsMajor = 0
if (Test-Path $vswhere) {
    $installs = & $vswhere -all -products * -requires Microsoft.VisualStudio.Component.VC.Tools.x86.x64 -format json | ConvertFrom-Json
    $best = $installs | ForEach-Object { [pscustomobject]@{ Path = $_.installationPath; Major = [int]($_.installationVersion.Split('.')[0]) } } |
        Sort-Object { switch ($_.Major) { 17 { 0 } 16 { 1 } default { 2 } } }, Major | Select-Object -First 1
    if ($best) {
        $cand = Join-Path $best.Path "VC\Auxiliary\Build\vcvars64.bat"
        if (Test-Path $cand) { $vcvars = $cand; $vsMajor = $best.Major }
    }
}
if (-not $vcvars) {
    Write-Host "!! Visual Studio C++ Build Tools not found. Install 'Desktop development with C++' (winget install Microsoft.VisualStudio.2022.BuildTools), then run setup again."
    exit 4
}
Write-Host "C++ toolchain: $vcvars (VS major $vsMajor)"
cmd /c "`"$vcvars`" >nul 2>&1 && set" | ForEach-Object {
    if ($_ -match '^([^=]+)=(.*)$') { [Environment]::SetEnvironmentVariable($Matches[1], $Matches[2], "Process") }
}
if (-not (Get-Command cl -ErrorAction SilentlyContinue)) { Write-Host "!! cl.exe still not on PATH after loading vcvars"; exit 4 }

$env:CUDA_HOME = $cuda; $env:CUDA_PATH = $cuda
$env:PATH = "$cuda\bin;$env:PATH"
$env:DISTUTILS_USE_SDK = "1"; $env:MSSdk = "1"
$env:MAX_JOBS = "4"
if ($vsMajor -gt 17) { $env:NVCC_APPEND_FLAGS = "-allow-unsupported-compiler" }
$arch = (& $py -c "import torch;c=torch.cuda.get_device_capability();print(f'{c[0]}.{c[1]}')" 2>$null)
if (-not $arch) { $arch = "8.9" }   # RTX 40-series default
$env:TORCH_CUDA_ARCH_LIST = $arch
Write-Host "Compiling for GPU architecture sm_$($arch.Replace('.',''))"

# ---------------------------------------------------------------- 5. native extensions
New-Item -ItemType Directory -Force $extDir | Out-Null

# Upstream TRELLIS.2 and its CUDA extensions are developed on Linux. These small, idempotent source
# patches are what make them compile with MSVC. Each only applies if its pattern is still present.
function Patch-File([string]$file, [string]$pattern, [string]$replacement, [string]$why) {
    if (-not (Test-Path $file)) { Write-Host "!! patch target missing: $file"; return }
    $text = [IO.File]::ReadAllText($file)
    $new = [regex]::Replace($text, $pattern, $replacement)
    if ($new -ne $text) {
        [IO.File]::WriteAllText($file, $new, (New-Object Text.UTF8Encoding $false))
        Write-Host "patched $(Split-Path $file -Leaf): $why"
    }
}
function Apply-Patches([string]$name, [string]$dir) {
    switch ($name) {
        "CuMesh" {
            # With MSVC strict-conformance mode (/permissive-), once ATen is included `::cuda` (CCCL) is reported
            # ambiguous with c10::cuda (error C2872). Verified with a 4-line repro: same file compiles without the flag.
            git -C $dir checkout -- . 2>&1 | Out-Null   # pristine sources so patching is deterministic
            Patch-File (Join-Path $dir "setup.py") '\s*"(?:-Xcompiler=)?/permissive-",' '' "drop /permissive- (MSVC C2872 'cuda' ambiguous with ATen)"
        }
        "FlexGEMM" {
            # nvcc front end rejects `x.data_ptr<T>()` where T is a type alias inside a generic lambda.
            Patch-File (Join-Path $dir "flex_gemm\kernels\cuda\spconv\sparse_neighbor_map.cu") '(\w+)\.data_ptr<T>\(\)' 'reinterpret_cast<T*>($1.data_ptr())' "data_ptr<T>() inside generic lambda"
        }
        "o-voxel" {
            git -C $repo checkout -- o-voxel 2>&1 | Out-Null   # pristine sources before patching
            # `1e-6d` / `0.0d` is a GCC extension, not valid C++.
            Patch-File (Join-Path $dir "src\convert\flexible_dual_grid.cpp") '(\b\d+(?:\.\d*)?(?:[eE][+-]?\d+)?)d\b' '$1' "invalid 'd' literal suffix"
            # `torch::zeros({N, C})` with size_t N,C is a narrowing conversion MSVC rejects (GCC only warns).
            foreach ($f in Get-ChildItem (Join-Path $dir "src") -Recurse -Filter *.cpp) {
                Patch-File $f.FullName 'torch::(zeros|empty|ones)\(\{([A-Za-z_]\w*), ([A-Za-z_]\w*)\}' 'torch::$1({(int64_t)$2, (int64_t)$3}' "size_t -> int64_t in tensor shape"
                Patch-File $f.FullName 'from_blob\(([^,]+), \{([A-Za-z_]\w*\.size\(\))\}' 'from_blob($1, {(int64_t)$2}' "size_t -> int64_t in from_blob shape"
            }
            # setup.py passes GCC-only flags to MSVC (silently ignored -> C++14 -> compile errors).
            $sp = Join-Path $dir "setup.py"
            Patch-File $sp '"cxx": \["-O3", "-std=c\+\+17"\]' '"cxx": (["/O2", "/std:c++17", "/EHsc", "/Zc:__cplusplus"] if __import__("sys").platform == "win32" else ["-O3", "-std=c++17"])' "MSVC flags for the C++ compiler (no /permissive-: upstream relies on lenient narrowing)"
            Patch-File $sp '"nvcc": \["-O3","-std=c\+\+17"\] \+ cc_flag' '"nvcc": ["-O3","-std=c++17"] + (["-Xcompiler=/std:c++17", "-Xcompiler=/EHsc", "-Xcompiler=/Zc:__cplusplus"] if __import__("sys").platform == "win32" else []) + cc_flag' "MSVC host flags for nvcc"
        }
    }
}

function Build-Ext([string]$name, [string]$url, [string]$branch, [string]$module) {
    if ($Only.Count -gt 0 -and $Only -notcontains $name) { return }
    if ($Only.Count -eq 0) {
        & $py -c "import $module" 2>$null
        if ($LASTEXITCODE -eq 0) { Write-Host "$name already installed, skipping"; return }
    }
    $dir = Join-Path $extDir $name
    if ($name -eq "o-voxel") { $dir = Join-Path $repo "o-voxel" }
    elseif (-not (Test-Path (Join-Path $dir "setup.py")) -and -not (Test-Path (Join-Path $dir "pyproject.toml"))) {
        if (Test-Path $dir) { Remove-Item -Recurse -Force $dir }
        $gitArgs = @("clone", "--recursive", "--depth", "1")
        if ($branch) { $gitArgs += @("-b", $branch) }
        $gitArgs += @($url, $dir)
        if (-not (Exec "clone $name" { git @gitArgs })) { return }
    }
    Apply-Patches $name $dir
    # --no-deps: dependencies are installed above; some extensions otherwise try to fetch siblings from git without submodules.
    Exec "build $name" { PipInstall --no-build-isolation --no-deps $dir } | Out-Null
}
Build-Ext "nvdiffrast" "https://github.com/NVlabs/nvdiffrast.git" "v0.4.0" "nvdiffrast.torch"
Build-Ext "nvdiffrec" "https://github.com/JeffreyXiang/nvdiffrec.git" "renderutils" "nvdiffrec_render"
Build-Ext "CuMesh" "https://github.com/JeffreyXiang/CuMesh.git" $null "cumesh"
Build-Ext "FlexGEMM" "https://github.com/JeffreyXiang/FlexGEMM.git" $null "flex_gemm"
Build-Ext "o-voxel" $null $null "o_voxel"

# ---------------------------------------------------------------- 6. verify
Step "verify imports"
$env:PYTHONPATH = $repo
$code = "import torch, nvdiffrast.torch, cumesh, flex_gemm, o_voxel; print('torch', torch.__version__, 'cuda', torch.version.cuda, 'gpu', torch.cuda.is_available()); print('native extensions OK')"
if (-not (Exec "import check" { & $py -c $code })) { }

if ($failed.Count -gt 0) {
    Write-Host ""
    Write-Host "Setup finished with problems:"
    $failed | ForEach-Object { Write-Host "  - $_" }
    Write-Host "Logs: $logs"
    exit 3
}
Write-Host ""
Write-Host "Setup complete."
