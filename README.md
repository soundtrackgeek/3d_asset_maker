# TRELLIS.2 Studio

Desktop studio and agent-first CLI that turn images into game-ready 3D assets (GLB/OBJ/PLY) with [Microsoft TRELLIS.2](https://github.com/microsoft/TRELLIS.2), tuned for a **16 GB RTX 4060 Ti on Windows**. Built with Rust and Tauri 2.

Outputs target **three.js, Unity (glTFast), Unreal Engine (Interchange glTF), Godot** and anything else that reads glTF.

## Install

Download `TRELLIS-Studio.exe` (GUI) and `t2.exe` (CLI) from the [latest release](https://github.com/soundtrackgeek/3d_asset_maker/releases/latest). The GUI checks for new releases on start, every 30 minutes, and via **Check for updates**.

### TRELLIS.2 runtime (one time)

TRELLIS.2 is officially Linux/24 GB only, so the studio manages its own Windows runtime:

```powershell
t2 setup      # or: GUI > Setup & diagnostics > Install
t2 doctor     # verify GPU, runtime and HF token
```

Requirements: NVIDIA driver, `git`, [`uv`](https://docs.astral.sh/uv/) (or the `py` launcher with Python 3.12), plus Visual Studio Build Tools (Desktop C++) and CUDA Toolkit 12.8 for the native extensions (nvdiffrast, CuMesh, FlexGEMM, o-voxel). The installer is best-effort and lists what failed. Some TRELLIS.2 pieces are gated on Hugging Face (background removal, DINOv3): put `HF_TOKEN=...` in `.env.local` next to the exe or in the working directory.

Until the runtime is installed, use `--mock` (GUI: "Mock backend") to exercise the full workflow with a placeholder cube.

## CLI (for agents)

Stdout is always one JSON document; progress goes to stderr. Exit codes: `0` ok, `1` error, `2` a job failed.

```powershell
t2 options                                   # presets, engines, defaults (discover capabilities)
t2 doctor                                    # readiness + recommended preset
t2 generate samples\chest.png --preset balanced --engine unity --format glb,obj --out out
t2 generate a.png b.png --preset draft --seed 7 --no-bg-removal
t2 generate x.png --options-file overrides.json   # any field from `t2 options`
t2 generate x.png --mock                     # GPU-free dry run
t2 jobs list | show <id> | delete <id>
t2 update check
```

Each export folder contains `model.glb` (+ requested formats) and `asset.json` (options, seed, units, engine notes). `TRELLIS_STUDIO_HOME` moves the data folder (default `%LOCALAPPDATA%\TrellisStudio`).

### Presets (16 GB)

| Preset | Pipeline | Textures | Tris | Notes |
|---|---|---|---|---|
| `draft` | 512 | 1K | 50k | fast check |
| `balanced` | 1024 cascade | 2K | 150k | default |
| `high` | 1536 cascade | 4K | 500k | may OOM on 16 GB |

Engine targets only ever lower the budget: `threejs` 150k/2K, `unity` and `godot` 100k/2K, `unreal` 200k/4K.

## Development

```powershell
cargo build --release      # target\release\trellis-studio.exe and t2.exe
cargo test -p trellis-core
```

Layout: `crates/core` (shared engine, Python worker, installer), `crates/cli` (`t2`), `src-tauri` (GUI), `ui/` (plain HTML/JS). Releases: `scripts/release.ps1 -Version X.Y.Z`. See `AGENTS.md` for the workflow.

## Status

The queue, UI, CLI, updater and mock export are tested. The real TRELLIS.2 path (`worker.py`, `setup.ps1`) is written against the upstream API but has **not yet been run end to end on a 4060 Ti**; expect to adjust the installer and low-VRAM behaviour on first real run.

Sample inputs in `samples/` were generated with Codex image generation.
