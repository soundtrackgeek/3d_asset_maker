## Development Workflow

**IMPORTANT**: After any code change, bug fix, or feature addition/removal, you MUST complete all of these steps:

1. **Update README.md** if the change affects:
   - Usage examples or commands
   - Installation instructions
   - Configuration options
   - Available features

2. **Update CHANGELOG.md**:
   - Add new version number following semantic versioning (MAJOR.MINOR.PATCH)
   - Add entry under appropriate category (Added, Changed, Fixed, Removed)
   - Include date in format YYYY-MM-DD

3. **Commit and push changes**:
   - Use `git add` to stage all modified files (README.md, CHANGELOG.md, and code files)
   - Create descriptive commit message following existing style
   - Push to remote repository with `git push`

4. **Publish the .exe for every version bump**:
   - Run `powershell -File scripts/release.ps1 -Version X.Y.Z` (CHANGELOG.md must already contain a `## [X.Y.Z]` section).
   - It bumps the version, commits, tags `vX.Y.Z` and pushes. The `Release` GitHub Actions workflow then builds the NSIS installer on a clean runner and creates the GitHub Release with `TRELLIS-Studio-Setup.exe` and `t2.exe` attached; the script waits for it. `-Local` builds and publishes from this PC instead (needs `cargo install tauri-cli --version "^2"`). The updater looks for the `*-Setup.exe` asset name, so keep it. The in-app updater reads this release, so never skip it. Binaries live in Releases, not in git history.

5. **Run `cargo clean` after every push** (`scripts/release.ps1` does this automatically). The `target/` folder is large; do not leave it around.

## Project notes

- Rust workspace: `crates/core` (shared engine), `crates/cli` (`t2` binary, agent-first, JSON on stdout), `src-tauri` (GUI), `ui/` (plain HTML/JS, no npm build).
- Target hardware: RTX 4060 Ti 16 GB on Windows. Defaults must stay safe for 16 GB (xformers attention, low-VRAM offload, `draft`/512 preset; the 1024 cascade overflows 16 GB VRAM at decode and peaks ~18 GB of system RAM). TRELLIS.2's sparse attention has no sdpa path, so never default to it.
- Upstream TRELLIS.2 and its CUDA extensions are Linux-only. The MSVC compatibility patches live in `crates/core/python/setup.ps1` (`Apply-Patches`), applied to pristine checkouts. To iterate on one extension: `powershell -File crates/core/python/setup.ps1 -Root "$env:LOCALAPPDATA\TrellisStudio\runtime" -Only CuMesh`. `scripts/dev/nvcc-probe.ps1` compiles a tiny `.cu` in the same build environment to bisect compiler errors.
- Version pins that matter: torch 2.6.0+cu124, transformers 4.x (5.x breaks DINOv3), xformers 0.0.29.post3 with triton-windows 3.2.x, CUDA 12.4 components.
- Never use NSIS `ReadRegStr` to edit PATH (returns empty over ~1 KB); the installer calls `t2 path add|remove` instead.
- `HF_TOKEN` lives in `.env.local` (git-ignored). Never print or commit it.
- Test without a GPU using `t2 generate <img> --mock`.
- Node comes from fnm if ever needed (see global notes); the project itself needs no Node.