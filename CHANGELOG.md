# Changelog

All notable changes to this project are documented here. Format: [Keep a Changelog](https://keepachangelog.com/), versioning: [SemVer](https://semver.org/).

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
