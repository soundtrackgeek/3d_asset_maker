# TRELLIS.2 runtime installer for Windows (NVIDIA, CUDA 12.8 wheels).
# Creates <Root>\venv and clones microsoft/TRELLIS.2 into <Root>\TRELLIS.2.
# Pure-Python deps are required; the native CUDA extensions need
# Visual Studio Build Tools (C++) and the CUDA Toolkit and are best-effort.
param([Parameter(Mandatory = $true)][string]$Root, [string]$PythonVersion = "3.12")
$ErrorActionPreference = "Continue"
New-Item -ItemType Directory -Force $Root | Out-Null
Set-Location $Root
$venv = Join-Path $Root "venv"
$py = Join-Path $venv "Scripts\python.exe"
$failed = @()

function Step($m) { Write-Host "==> $m" }
function Try-Run($label, [scriptblock]$cmd) {
    Step $label
    & $cmd
    if ($LASTEXITCODE -ne 0) { Write-Host "!! FAILED: $label"; $script:failed += $label }
}

if (-not (Test-Path $py)) {
    if (Get-Command uv -ErrorAction SilentlyContinue) {
        Try-Run "create venv (uv, Python $PythonVersion)" { uv venv --python $PythonVersion $venv }
    }
    elseif (Get-Command py -ErrorAction SilentlyContinue) {
        Try-Run "create venv (py launcher, $PythonVersion)" { py "-$PythonVersion" -m venv $venv }
    }
    else { Write-Host "!! Need 'uv' (winget install astral-sh.uv) or the 'py' launcher with Python $PythonVersion"; exit 1 }
}
if (-not (Test-Path $py)) { Write-Host "!! venv was not created"; exit 1 }

if (-not (Test-Path (Join-Path $Root "TRELLIS.2"))) {
    Try-Run "clone TRELLIS.2" { git clone -b main --recursive https://github.com/microsoft/TRELLIS.2.git (Join-Path $Root "TRELLIS.2") }
}

$pipInstall = if (Get-Command uv -ErrorAction SilentlyContinue) { @("uv", "pip", "install", "--python", $py) } else { @($py, "-m", "pip", "install") }
function Pip { & $pipInstall[0] ($pipInstall[1..($pipInstall.Length - 1)] + $args) }

Try-Run "install PyTorch (CUDA 12.8)" { Pip torch torchvision --index-url https://download.pytorch.org/whl/cu128 }
Try-Run "install Python dependencies" {
    Pip pillow imageio imageio-ffmpeg tqdm easydict opencv-python-headless ninja trimesh transformers tensorboard pandas lpips zstandard kornia timm huggingface_hub safetensors "numpy<2.3"
    Pip "git+https://github.com/EasternJournalist/utils3d.git"
}

# Native extensions (need MSVC Build Tools + CUDA Toolkit). Best effort.
$ext = Join-Path $Root "extensions"
New-Item -ItemType Directory -Force $ext | Out-Null
$repo = Join-Path $Root "TRELLIS.2"
Try-Run "nvdiffrast"  { Pip --no-build-isolation "git+https://github.com/NVlabs/nvdiffrast.git" }
Try-Run "CuMesh"      { Pip --no-build-isolation "git+https://github.com/JeffreyXiang/CuMesh.git" }
Try-Run "FlexGEMM"    { Pip --no-build-isolation "git+https://github.com/JeffreyXiang/FlexGEMM.git" }
Try-Run "o-voxel"     { Pip --no-build-isolation (Join-Path $repo "o-voxel") }

Step "Hugging Face login check (gated models need HF_TOKEN in .env.local)"
if ($failed.Count -gt 0) {
    Write-Host ""
    Write-Host "Setup finished with problems:"
    $failed | ForEach-Object { Write-Host "  - $_" }
    Write-Host "Install 'Visual Studio Build Tools' (Desktop C++) and CUDA Toolkit 12.8, then re-run setup."
    exit 3
}
Write-Host "Setup complete."
