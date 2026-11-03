# TRELLIS.2 Studio

Desktop studio and agent-first CLI that turn images into game-ready 3D assets (GLB/OBJ/PLY) with [Microsoft TRELLIS.2](https://github.com/microsoft/TRELLIS.2), tuned for a **16 GB RTX 4060 Ti on Windows**. Built with Rust and Tauri 2.

Outputs target **three.js, Unity (glTFast), Unreal Engine (Interchange glTF), Godot** and anything else that reads glTF.

## Install

Run `TRELLIS-Studio-Setup.exe` from the [latest release](https://github.com/soundtrackgeek/3d_asset_maker/releases/latest). It installs per user (no admin rights) to `%LOCALAPPDATA%\TRELLIS Studio` (you can pick another folder; reinstalls reuse the last one), creates shortcuts, and adds the folder to your `PATH` so `t2` works in any new shell. The release also has a standalone `t2.exe` for headless or CI machines.

The app checks for new releases on start, every 30 minutes, and via **Check for updates**; **Download & install** fetches and runs the new installer (`t2 update install` does the same silently).

### TRELLIS.2 runtime (one time)

TRELLIS.2 is officially Linux/24 GB only, so the studio manages its own Windows runtime:

```powershell
t2 setup      # or: GUI > Setup & diagnostics > Install
t2 doctor     # verify GPU, runtime and HF token
```

Requirements you install yourself: an NVIDIA driver, `git`, [`uv`](https://docs.astral.sh/uv/) (or the `py` launcher with Python 3.12), and **Visual Studio Build Tools with the C++ workload** (2019 or 2022 preferred; the installer picks a supported one automatically).

What setup does for you (about 1.2 GB of NVIDIA downloads plus roughly 10 GB for PyTorch and the model, 20-40 minutes the first time):
- creates a Python 3.12 venv with PyTorch 2.6.0 (CUDA 12.4, the combination upstream tests) and xformers (the only attention backend usable on Windows for TRELLIS.2's sparse layers);
- downloads the CUDA 12.4 compiler and dev libraries from NVIDIA into the app's data folder, with checksums, no admin rights and no system-wide install (an existing CUDA 12.x install is used if found);
- builds nvdiffrast, nvdiffrec, CuMesh, FlexGEMM and o-voxel with a handful of small MSVC compatibility patches (upstream is Linux-only), then imports all of them to verify. Build logs are in `<data folder>\runtime\logs`.

**Hugging Face:** put `HF_TOKEN=...` in `.env.local` (next to the exe or in the working directory). Two models are gated, so accept their licences with that account first: [facebook/dinov3-vitl16-pretrain-lvd1689m](https://huggingface.co/facebook/dinov3-vitl16-pretrain-lvd1689m) (required) and [briaai/RMBG-2.0](https://huggingface.co/briaai/RMBG-2.0) (background removal; if you have no access the worker falls back to the open `ZhengPeng7/BiRefNet`, same architecture).

Until the runtime is installed, use `--mock` (GUI: "Mock backend") to exercise the full workflow with a placeholder cube.

## CLI (for agents)

Stdout is always one JSON document; progress goes to stderr. Exit codes: `0` ok, `1` error, `2` a job failed.

```powershell
t2 options                                   # presets, engines, defaults (discover capabilities)
t2 doctor                                    # readiness + recommended preset
t2 generate samples\chest.png --preset draft --engine unity --format glb,obj --out out
t2 generate a.png b.png --preset draft --seed 7 --no-bg-removal
t2 generate sword.png --tries 4                   # 4 seeds, all kept as model_seed<N>.glb
t2 jobs use <job-id> 43                           # promote seed 43 to model.glb
t2 generate x.png --options-file overrides.json   # any field from `t2 options`
t2 generate x.png --mock                     # GPU-free dry run
t2 jobs list | show <id> | delete <id>
t2 update check | install
t2 path add|remove <dir>                     # user PATH edit used by the installer (backs up the old value)
```

Each export folder contains `model.glb` (+ requested formats) and `asset.json` (options, seed, units, engine notes). `TRELLIS_STUDIO_HOME` moves the data folder (default `%LOCALAPPDATA%\TrellisStudio`).

### Presets (16 GB)

| Preset | Pipeline | Textures | Tris | Notes |
|---|---|---|---|---|
| `draft` | 512 | 1K | 50k | **default**. Measured on an RTX 4060 Ti 16 GB: ~106 s, 5.5 GB VRAM, 9 GB RAM peak |
| `balanced` | 1024 cascade | 2K | 150k | needs a 24 GB+ card: on 16 GB the decode spills into system RAM (~473 s, ~18 GB RAM peak) |
| `high` | 1536 cascade | 4K | 500k | 32 GB+ VRAM; will OOM on 16 GB |

Thin or glassy objects (swords, crystals) can come out with stray sheets or strings for some seeds. Try another seed or use `--tries N` and pick the clean one.

The worker is memory-guarded: it aborts with a clear error if its resident RAM passes 60% of the PC's RAM (override with `T2_MAX_RAM_MB`).

Engine targets only ever lower the budget: `threejs` 150k/2K, `unity` and `godot` 100k/2K, `unreal` 200k/4K.

## Development

```powershell
cargo build --release      # target\release\trellis-studio.exe and t2.exe
cargo test -p trellis-core
cargo install tauri-cli --version "^2"   # once, for the installer
cargo tauri build --bundles nsis         # target\release\bundle\nsis\*-setup.exe
powershell -File scripts\build-dist.ps1  # both of the above into dist\ (what CI runs)
powershell -File scripts\release.ps1 -Version X.Y.Z   # bump, tag, push; GitHub Actions publishes the Release
```

Layout: `crates/core` (shared engine, Python worker, installer), `crates/cli` (`t2`), `src-tauri` (GUI), `ui/` (plain HTML/JS). Releases: `scripts/release.ps1 -Version X.Y.Z`. See `AGENTS.md` for the workflow.

## Status

Verified on an RTX 4060 Ti 16 GB (Windows 11): the full runtime install, and real image-to-3D generation through both the CLI and the GUI. `samples/chest.glb` is the real output of `t2 generate samples/chest.png --preset draft --engine unity` (48k triangles, 1024 px texture, about 5.5 minutes including model load); `samples/real-chest-viewer.png` shows it in the app.

Sample inputs in `samples/` were generated with Codex image generation.
