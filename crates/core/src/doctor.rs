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
    add(
        "hf_token",
        env::hf_token().is_some(),
        if env::hf_token().is_some() { "HF_TOKEN set".into() } else { "HF_TOKEN missing".into() },
        Some("Put HF_TOKEN=... in .env.local. Needed for gated models (RMBG-2.0 background removal, DINOv3)."),
    );
    let ready = py.exists() && repo.exists() && vram.is_some();
    let rec = match vram {
        Some(v) if v >= 22_000 => "high",
        Some(v) if v >= 15_000 => "balanced",
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
