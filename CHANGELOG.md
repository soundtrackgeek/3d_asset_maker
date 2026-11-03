# Changelog

All notable changes to this project are documented here. Format: [Keep a Changelog](https://keepachangelog.com/), versioning: [SemVer](https://semver.org/).

## [0.2.2] - 2026-11-03

First release where real TRELLIS.2 generation works end to end on a 16 GB RTX 4060 Ti under Windows.

### Fixed
- Runtime install failed for all native extensions. The installer now loads the MSVC and CUDA build environment itself, clones with submodules at the versions upstream pins, builds with `--no-build-isolation --no-deps`, and verifies by importing everything.
- No system CUDA Toolkit needed: the compiler and dev libraries (CUDA 12.4, checksum-verified) are downloaded from NVIDIA's redistributable zips into the app data folder. The full NVIDIA installer failed on this machine because of its bundled driver/ShadowPlay components.
- Windows compile fixes for upstream sources, applied idempotently: CuMesh (`/permissive-` makes MSVC report `cuda` ambiguous with `c10::cuda`), FlexGEMM (`data_ptr<T>()` in a generic lambda), o-voxel (GCC-only `1e-6d` literals, narrowing `size_t` shapes, GCC-only compiler flags).
- Pinned PyTorch 2.6.0+cu124 (tested upstream), `transformers` 4.x (5.x breaks DINOv3) and xformers 0.0.29 with matching `triton-windows` 3.2.
- Default attention backend is now `xformers`. TRELLIS.2's sparse attention does not implement `sdpa`, so the previous default could never run.
- Worker falls back to the open `ZhengPeng7/BiRefNet` background remover when the account has no access to the gated `briaai/RMBG-2.0`.

### Changed
- Default preset is now `draft` (512 pipeline). Measured on an RTX 4060 Ti 16 GB: 106 s, 5.5 GB VRAM, 9 GB RAM. `balanced` (1024 cascade) overflows 16 GB VRAM while decoding (27 GB reserved, ~18 GB RAM peak, 473 s), so it is now documented as a 24 GB+ preset and `t2 doctor` recommends it only from 22 GB.

### Added
- Memory guard: the worker aborts with a clear error when its resident RAM passes 60% of physical RAM (`T2_MAX_RAM_MB` overrides); the worker also runs in a Windows job object (commit backstop, killed with the host). Resident RAM is logged to `worker.log`.
- Lower RAM use: flow models are loaded per stage and evicted afterwards (was all at once, ~17 GB), unused resolutions are not loaded, and models that can't be built lazily are built on the GPU so init temporaries never fragment the CPU heap.
- Peak VRAM per job (`peak_vram_mb`, shown in the app), per-stage CUDA cache release, `PYTHONFAULTHANDLER` for native crash traces, and `scripts/dev/gen-with-vram.ps1`, `ram-profile.py`.
- `t2 doctor` checks the CUDA toolkit, C++ build tools and whether the native extensions actually import; "ready" now requires that.
- `setup.ps1 -Only <extension>` to rebuild a single extension, build logs in `runtime\logs`, and `scripts/dev/nvcc-probe.ps1` to bisect compiler errors.
- `samples/chest.glb` (real output) and a viewer screenshot.

## [0.2.1] - 2026-11-02

### Added
- GitHub Actions: `release.yml` builds the installer on a clean Windows runner and publishes the Release when a `vX.Y.Z` tag is pushed (manual runs are a dry run that uploads artifacts); `ci.yml` runs unit tests, builds, and smoke-tests the mock backend on every push and PR.
- `scripts/build-dist.ps1` and `scripts/release-notes.ps1`, shared by CI and the local release script.

### Changed
- `scripts/release.ps1` now bumps, commits, tags and pushes, then waits for CI to publish. `-Local` keeps the old build-and-publish-from-this-PC behaviour as a fallback.

### Fixed
- README stated the wrong default install folder; it is `%LOCALAPPDATA%\TRELLIS Studio`. The installer remembers the last folder in the registry (standard Tauri behaviour), so reinstalls reuse it.

## [0.2.0] - 2026-11-02

### Added
- Windows installer (`TRELLIS-Studio-Setup.exe`, NSIS, per-user, no admin rights): Start menu and desktop shortcuts, uninstaller, WebView2 bootstrap, and bundled `t2.exe`.
- The install folder is added to the user `PATH` (and removed on uninstall) so agents can call `t2` from any shell. Done by `t2 path add|remove`, which preserves the registry value type, backs up the old value to `user-path-backup.txt` and never writes a value it could not read (verified against a 1.2 KB PATH, install and uninstall restore it byte for byte).
- In-app "Download & install" for updates: downloads the new installer from this repo's GitHub release, launches it and quits.
- `t2 update install`: silent CLI update.

### Changed
- Release assets are now `TRELLIS-Studio-Setup.exe` and a standalone `t2.exe` (for headless/CI use) instead of the bare `TRELLIS-Studio.exe`.
- `scripts/release.ps1` builds the installer with `cargo tauri build --bundles nsis`.

## [0.1.0] - 2026-11-02

### Added
- Tauri 2 desktop app "TRELLIS.2 Studio": drag-and-drop image import, job queue with live progress, in-app GLB viewer, per-job options (presets, resolution, seed, triangle budget, texture size, remesh, formats, sampler overrides, low-VRAM and attention-backend switches).
- Presets tuned for a 16 GB RTX 4060 Ti: `draft`, `balanced` (default, 1024 cascade) and `high` (1536 cascade, experimental).
- Engine targets (`threejs`, `unity`, `unreal`, `godot`) that cap triangle and texture budgets and write an `asset.json` manifest next to each export.
- Agent-first `t2` CLI: every command prints one JSON document on stdout (`doctor`, `setup`, `options`, `generate`, `jobs`, `update check`).
- Shared `trellis-core` crate used by both GUI and CLI, so headless runs behave identically.
- Python worker + Windows installer script for the TRELLIS.2 runtime (SDPA attention, CUDA 12.8 PyTorch).
- Mock backend (`--mock`) that writes a textured cube GLB for GPU-free testing of the whole workflow.
- Update checker against GitHub Releases: on startup, every 30 minutes while open, and a manual button (GUI) or `t2 update check` (CLI).
- `scripts/release.ps1`: version bump, build, tag, push, GitHub Release with the `.exe` files, then `cargo clean`.
- Sample test images in `samples/` generated with Codex image generation.
