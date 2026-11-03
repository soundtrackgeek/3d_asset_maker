use crate::{env, paths};
use serde::Serialize;
use std::process::Command;

#[derive(Serialize, Clone, Debug)]
pub struct Check {
    pub name: String,
    pub ok: bool,
    pub detail: String,
    pub hint: Option<String>,
}

#[derive(Serialize, Clone, Debug)]
pub struct Report {
    pub gpu: Option<String>,
    pub vram_mb: Option<u64>,
    pub recommended_preset: String,
    pub ready_for_real_generation: bool,
    pub home: String,
    pub checks: Vec<Check>,
}

fn nvidia_smi() -> Option<(String, u64)> {
    let candidates = [
        "nvidia-smi".to_string(),
        r"C:\Windows\System32\nvidia-smi.exe".to_string(),
        r"C:\Program Files\NVIDIA Corporation\NVSMI\nvidia-smi.exe".to_string(),
    ];
    for c in candidates {
        let mut cmd = Command::new(&c);
        cmd.args(["--query-gpu=name,memory.total", "--format=csv,noheader,nounits"]);
        crate::runner::no_window(&mut cmd);
        if let Ok(o) = cmd.output() {
            if o.status.success() {
                let s = String::from_utf8_lossy(&o.stdout);
                let line = s.lines().next()?;
                let (name, mem) = line.rsplit_once(',')?;
                return Some((name.trim().to_string(), mem.trim().parse().ok()?));
            }
        }
    }
    None
}

/// Newest CUDA 12.x toolkit folder that contains nvcc.
fn find_cuda() -> Option<String> {
    let own = paths::runtime_dir().join(r"cuda\v12.4");
    if own.join(r"bin\nvcc.exe").exists() {
        return Some(own.display().to_string());
    }
    let base =std::path::PathBuf::from(std::env::var("ProgramFiles").unwrap_or_else(|_| r"C:\Program Files".into()))
        .join(r"NVIDIA GPU Computing Toolkit\CUDA");
    let mut dirs: Vec<_> = std::fs::read_dir(&base).ok()?.flatten().map(|e| e.path()).collect();
    dirs.sort();
    dirs.into_iter()
        .rev()
        .find(|d| d.join(r"bin\nvcc.exe").exists() && d.file_name().is_some_and(|n| n.to_string_lossy().starts_with("v12.")))
        .map(|d| d.display().to_string())
}

/// First Visual Studio install that has the C++ x64 tools (via vswhere).
fn find_vs() -> Option<String> {
    let vswhere = std::path::PathBuf::from(std::env::var("ProgramFiles(x86)").unwrap_or_else(|_| r"C:\Program Files (x86)".into()))
        .join(r"Microsoft Visual Studio\Installer\vswhere.exe");
    let mut cmd = Command::new(vswhere);
    cmd.args(["-all", "-products", "*", "-requires", "Microsoft.VisualStudio.Component.VC.Tools.x86.x64", "-property", "displayName"]);
    crate::runner::no_window(&mut cmd);
    let o = cmd.output().ok()?;
    let s = String::from_utf8_lossy(&o.stdout);
    let names: Vec<&str> = s.lines().filter(|l| !l.trim().is_empty()).collect();
    if names.is_empty() { None } else { Some(names.join(", ")) }
}

pub fn run() -> Report {
    let gpu = nvidia_smi();
    let mut checks = vec![];
    let mut add = |name: &str, ok: bool, detail: String, hint: Option<&str>| {
        checks.push(Check { name: name.into(), ok, detail, hint: hint.map(String::from) });
    };

    match &gpu {
        Some((n, m)) => add("nvidia_gpu", true, format!("{n}, {m} MiB"), None),
        None => add("nvidia_gpu", false, "nvidia-smi not found".into(), Some("Install the NVIDIA driver (TRELLIS.2 needs CUDA).")),
    }
    let vram = gpu.as_ref().map(|g| g.1);
    if let Some(v) = vram {
        let ok = v >= 15_000;
        add(
            "vram",
            ok,
            format!("{v} MiB"),
            if ok { None } else { Some("Under 16 GB: use --preset draft and expect OOM at higher resolutions.") },
        );
    }
    let py = paths::venv_python();
    add("python_venv", py.exists(), py.display().to_string(), Some("Run `t2 setup` to create the venv."));
    let repo = paths::trellis_repo();
    add("trellis2_repo", repo.exists(), repo.display().to_string(), Some("Run `t2 setup` to clone microsoft/TRELLIS.2."));
    // Build prerequisites for the native CUDA extensions.
    let cuda = find_cuda();
    add(
        "cuda_toolkit",
        cuda.is_some(),
        cuda.clone().unwrap_or_else(|| "nvcc (CUDA 12.x) not found".into()),
        Some("Setup downloads the CUDA 12.4 build components from NVIDIA (no admin needed), or install CUDA 12.x yourself."),
    );
    let vs = find_vs();
    add(
        "cpp_build_tools",
        vs.is_some(),
        vs.unwrap_or_else(|| "Visual Studio C++ Build Tools not found".into()),
        Some("Install 'Desktop development with C++' (winget install Microsoft.VisualStudio.2022.BuildTools)."),
    );
    // Actually importing the extensions is the only reliable proof the runtime works.
    let ext_ok = if py.exists() && repo.exists() {
        let mut cmd = Command::new(&py);
        cmd.args(["-c", "import torch, nvdiffrast.torch, cumesh, flex_gemm, o_voxel"]).env("PYTHONPATH", &repo);
        crate::runner::no_window(&mut cmd);
        cmd.output().map(|o| o.status.success()).unwrap_or(false)
    } else {
        false
    };
    add(
        "native_extensions",
        ext_ok,
        if ext_ok { "nvdiffrast, cumesh, flex_gemm, o_voxel import OK".into() } else { "not importable".into() },
        Some("Run `t2 setup`; build logs are in <data folder>\\runtime\\logs."),
    );
    add(
        "hf_token",
        env::hf_token().is_some(),
        if env::hf_token().is_some() { "HF_TOKEN set".into() } else { "HF_TOKEN missing".into() },
        Some("Put HF_TOKEN=... in .env.local. Needed for gated models (RMBG-2.0 background removal, DINOv3)."),
    );
    let ready = py.exists() && repo.exists() && vram.is_some() && ext_ok;
    let rec = match vram {
        Some(v) if v >= 32_000 => "high",
        Some(v) if v >= 22_000 => "balanced",
        _ => "draft",
    };
    Report {
        gpu: gpu.map(|g| g.0),
        vram_mb: vram,
        recommended_preset: rec.into(),
        ready_for_real_generation: ready,
        home: paths::home().display().to_string(),
        checks,
    }
}
