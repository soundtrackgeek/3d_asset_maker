use std::path::PathBuf;

/// Root folder for jobs, the Python runtime and settings.
/// Override with `TRELLIS_STUDIO_HOME`.
pub fn home() -> PathBuf {
    let p = std::env::var_os("TRELLIS_STUDIO_HOME")
        .map(PathBuf::from)
        .unwrap_or_else(|| {
            dirs::data_local_dir()
                .unwrap_or_else(|| PathBuf::from("."))
                .join("TrellisStudio")
        });
    let _ = std::fs::create_dir_all(&p);
    p
}

pub fn jobs_dir() -> PathBuf {
    let p = home().join("jobs");
    let _ = std::fs::create_dir_all(&p);
    p
}

/// Where setup creates the venv and clones TRELLIS.2.
pub fn runtime_dir() -> PathBuf {
    home().join("runtime")
}

pub fn venv_python() -> PathBuf {
    let r = runtime_dir().join("venv");
    if cfg!(windows) {
        r.join("Scripts").join("python.exe")
    } else {
        r.join("bin").join("python")
    }
}

pub fn trellis_repo() -> PathBuf {
    runtime_dir().join("TRELLIS.2")
}
