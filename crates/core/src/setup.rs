use crate::paths;
use anyhow::{anyhow, Result};
use std::io::{BufRead, BufReader};
use std::path::PathBuf;
use std::process::{Command, Stdio};

const WORKER_PY: &str = include_str!("../python/worker.py");
const SETUP_PS1: &str = include_str!("../python/setup.ps1");

/// Write the embedded worker + installer next to the runtime. Returns the worker path.
/// Re-extracted on every run so app updates always ship a matching worker.
pub fn extract_runtime_files() -> Result<PathBuf> {
    let dir = paths::runtime_dir();
    std::fs::create_dir_all(&dir)?;
    std::fs::write(dir.join("setup.ps1"), SETUP_PS1)?;
    let w = dir.join("worker.py");
    std::fs::write(&w, WORKER_PY)?;
    Ok(w)
}

/// Run the Windows installer, streaming every output line to `on_line`.
pub fn run_setup(on_line: &mut dyn FnMut(&str)) -> Result<()> {
    extract_runtime_files()?;
    let script = paths::runtime_dir().join("setup.ps1");
    let mut cmd = Command::new("powershell");
    cmd.args(["-NoProfile", "-ExecutionPolicy", "Bypass", "-File"])
        .arg(&script)
        .arg("-Root")
        .arg(paths::runtime_dir())
        .stdout(Stdio::piped())
        .stderr(Stdio::inherit());
    crate::runner::no_window(&mut cmd);
    let mut child = cmd.spawn().map_err(|e| anyhow!("cannot start powershell: {e}"))?;
    for l in BufReader::new(child.stdout.take().unwrap()).lines().map_while(|l| l.ok()) {
        on_line(&l);
    }
    let st = child.wait()?;
    if st.success() {
        Ok(())
    } else {
        Err(anyhow!("setup exited with {st}"))
    }
}
