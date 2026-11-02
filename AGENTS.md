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
- Target hardware: RTX 4060 Ti 16 GB on Windows. Defaults must stay safe for 16 GB (SDPA attention, low-VRAM offload, 1024 cascade).
- `HF_TOKEN` lives in `.env.local` (git-ignored). Never print or commit it.
- Test without a GPU using `t2 generate <img> --mock`.
- Node comes from fnm if ever needed (see global notes); the project itself needs no Node.