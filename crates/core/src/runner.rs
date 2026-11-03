use crate::job::{Job, Status};
use crate::{env, mock, paths, setup};
use anyhow::{anyhow, Result};
use serde_json::{json, Value};
use std::io::{BufRead, BufReader};
use std::process::{Command, Stdio};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc;
use std::time::{Duration, Instant};

type OnUpdate<'a> = &'a mut dyn FnMut(&Job);

/// Run a job to completion (blocking). Persists state after every progress event.
/// The caller is responsible for serialising GPU jobs (one at a time).
pub fn run(job: &mut Job, cancel: &AtomicBool, on_update: OnUpdate) -> Result<()> {
    let start = Instant::now();
    job.status = Status::Running;
    job.error = None;
    job.outputs.clear();
    job.candidates.clear();
    job.progress = 0.0;
    job.stage = "starting".into();
    update(job, on_update, start);

    let res = if job.mock { run_mock(job, cancel, on_update, start) } else { run_real(job, cancel, on_update, start) };

    job.elapsed_secs = start.elapsed().as_secs_f32();
    match &res {
        Ok(()) if cancel.load(Ordering::Relaxed) => {
            job.status = Status::Cancelled;
            job.stage = "cancelled".into();
        }
        Ok(()) => {
            job.status = Status::Done;
            job.progress = 1.0;
            job.stage = "done".into();
            let _ = write_manifest(job);
        }
        Err(e) => {
            job.status = Status::Failed;
            job.stage = "failed".into();
            job.error = Some(format!("{e:#}"));
        }
    }
    job.save()?;
    on_update(job);
    res.map(|_| ())
}

fn update(job: &mut Job, on_update: OnUpdate, start: Instant) {
    job.elapsed_secs = start.elapsed().as_secs_f32();
    let _ = job.save();
    on_update(job);
}

fn run_mock(job: &mut Job, cancel: &AtomicBool, on_update: OnUpdate, start: Instant) -> Result<()> {
    let stages = ["loading model (mock)", "sampling sparse structure", "sampling shape", "sampling texture", "exporting"];
    for (i, s) in stages.iter().enumerate() {
        if cancel.load(Ordering::Relaxed) {
            return Ok(());
        }
        job.stage = (*s).into();
        job.progress = i as f32 / stages.len() as f32;
        update(job, on_update, start);
        std::thread::sleep(Duration::from_millis(350));
    }
    let out = job.out_dir();
    std::fs::create_dir_all(&out)?;
    for f in job.options.formats.clone() {
        let name = format!("model.{f}");
        match f.as_str() {
            "glb" => mock::write_glb(&job.input_path(), &out.join(&name))?,
            "obj" => mock::write_obj(&out.join(&name))?,
            "ply" => mock::write_ply(&out.join(&name))?,
            _ => continue,
        }
        job.outputs.push(name);
    }
    Ok(())
}

fn run_real(job: &mut Job, cancel: &AtomicBool, on_update: OnUpdate, start: Instant) -> Result<()> {
    let py = paths::venv_python();
    let repo = paths::trellis_repo();
    if !py.exists() || !repo.exists() {
        return Err(anyhow!(
            "TRELLIS.2 runtime is not installed. Run `t2 setup` (or use Settings > Install in the app). For a GPU-free dry run use --mock."
        ));
    }
    let worker = setup::extract_runtime_files()?;
    let req = json!({
        "image": job.input_path(),
        "out_dir": job.out_dir(),
        "trellis_repo": repo,
        "options": job.options,
    });
    let req_path = job.dir().join("request.json");
    std::fs::write(&req_path, serde_json::to_vec_pretty(&req)?)?;
    let log = std::fs::File::create(job.dir().join("worker.log"))?;

    let mut cmd = Command::new(&py);
    cmd.arg(&worker)
        .arg(&req_path)
        .env("PYTHONUNBUFFERED", "1")
        .env("PYTHONFAULTHANDLER", "1") // native crashes (0xC0000005) get a Python stack in worker.log
        .env("PYTHONIOENCODING", "utf-8")
        // expandable_segments is not supported on Windows; these are, and curb fragmentation/over-reservation.
        .env("PYTORCH_CUDA_ALLOC_CONF", if cfg!(windows) { "garbage_collection_threshold:0.8,max_split_size_mb:512" } else { "expandable_segments:True" })
        .env("ATTN_BACKEND", &job.options.attn_backend)
        .env("PYTHONPATH", &repo)
        .stdout(Stdio::piped())
        .stderr(Stdio::from(log));
    if let Some(t) = env::hf_token() {
        cmd.env("HF_TOKEN", &t).env("HUGGING_FACE_HUB_TOKEN", t);
    }
    no_window(&mut cmd);
    let limit = ram_limit_mb();
    // The worker's own watchdog stops at `limit`; the job object cuts in a little later as a backstop.
    cmd.env("T2_MAX_RAM_MB", limit.to_string());
    let mut child = cmd.spawn().map_err(|e| anyhow!("failed to start python worker: {e}"))?;
    confine(&child, limit * 2); // commit backstop only; the worker watches resident RAM
    let stdout = child.stdout.take().unwrap();
    let (tx, rx) = mpsc::channel::<String>();
    std::thread::spawn(move || {
        for l in BufReader::new(stdout).lines().map_while(|l| l.ok()) {
            if tx.send(l).is_err() {
                break;
            }
        }
    });

    let mut worker_error: Option<String> = None;
    loop {
        if cancel.load(Ordering::Relaxed) {
            let _ = child.kill();
            let _ = child.wait();
            return Ok(());
        }
        match rx.recv_timeout(Duration::from_millis(200)) {
            Ok(line) => {
                if let Some(json) = line.strip_prefix("@@") {
                    if let Ok(v) = serde_json::from_str::<Value>(json) {
                        match v["event"].as_str() {
                            Some("progress") => {
                                if let Some(s) = v["stage"].as_str() {
                                    job.stage = s.into();
                                }
                                if let Some(p) = v["pct"].as_f64() {
                                    job.progress = p as f32;
                                }
                                update(job, on_update, start);
                            }
                            Some("output") => {
                                if let Some(f) = v["file"].as_str() {
                                    job.outputs.push(f.into());
                                }
                            }
                            Some("stat") => {
                                job.peak_vram_mb = v["peak_vram_mb"].as_u64().map(|m| m as u32);
                                update(job, on_update, start);
                            }
                            Some("candidate") => {
                                job.candidates.push(json!({"seed": v["seed"], "file": v["file"]}));
                                update(job, on_update, start);
                            }
                            Some("error") => worker_error = v["message"].as_str().map(String::from),
                            _ => {}
                        }
                    }
                }
            }
            Err(mpsc::RecvTimeoutError::Timeout) => {}
            Err(mpsc::RecvTimeoutError::Disconnected) => break,
        }
    }
    let status = child.wait()?;
    if !status.success() || job.outputs.is_empty() {
        let tail = std::fs::read_to_string(job.dir().join("worker.log"))
            .map(|s| s.lines().rev().take(12).collect::<Vec<_>>().into_iter().rev().collect::<Vec<_>>().join("\n"))
            .unwrap_or_default();
        return Err(anyhow!(
            "{}{}",
            worker_error.unwrap_or_else(|| format!("worker exited with {status}")),
            if tail.is_empty() { String::new() } else { format!("\n--- worker.log (tail) ---\n{tail}") }
        ));
    }
    Ok(())
}

/// RAM budget for the worker in MiB: `T2_MAX_RAM_MB`, else 60% of physical RAM (at least 8 GiB).
fn ram_limit_mb() -> u64 {
    if let Some(v) = std::env::var("T2_MAX_RAM_MB").ok().and_then(|v| v.parse().ok()) {
        return v;
    }
    #[cfg(windows)]
    unsafe {
        use windows_sys::Win32::System::SystemInformation::{GlobalMemoryStatusEx, MEMORYSTATUSEX};
        let mut m: MEMORYSTATUSEX = std::mem::zeroed();
        m.dwLength = std::mem::size_of::<MEMORYSTATUSEX>() as u32;
        if GlobalMemoryStatusEx(&mut m) != 0 {
            return (m.ullTotalPhys / (1 << 20) * 60 / 100).max(8192);
        }
    }
    16384
}

/// Hard backstop: put the worker in a Windows job object with a per-process commit limit
/// (allocations fail instead of exhausting the PC) that also kills the worker if the host dies.
#[cfg(windows)]
fn confine(child: &std::process::Child, limit_mb: u64) {
    use std::os::windows::io::AsRawHandle;
    use windows_sys::Win32::System::JobObjects::*;
    unsafe {
        let job = CreateJobObjectW(std::ptr::null(), std::ptr::null());
        if job.is_null() {
            return;
        }
        let mut info: JOBOBJECT_EXTENDED_LIMIT_INFORMATION = std::mem::zeroed();
        info.BasicLimitInformation.LimitFlags = JOB_OBJECT_LIMIT_PROCESS_MEMORY | JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE;
        info.ProcessMemoryLimit = (limit_mb as usize) << 20;
        SetInformationJobObject(
            job,
            JobObjectExtendedLimitInformation,
            &info as *const _ as *const _,
            std::mem::size_of::<JOBOBJECT_EXTENDED_LIMIT_INFORMATION>() as u32,
        );
        AssignProcessToJobObject(job, child.as_raw_handle() as _);
        // handle intentionally leaked: closing it (host exit) kills the worker
    }
}
#[cfg(not(windows))]
fn confine(_child: &std::process::Child, _limit_mb: u64) {}

#[cfg(windows)]
pub fn no_window(cmd: &mut Command) {
    use std::os::windows::process::CommandExt;
    cmd.creation_flags(0x0800_0000); // CREATE_NO_WINDOW
}
#[cfg(not(windows))]
pub fn no_window(_cmd: &mut Command) {}

/// `asset.json` next to the outputs: provenance + engine import notes for agents/tools.
fn write_manifest(job: &Job) -> Result<()> {
    let note = crate::options::ENGINES
        .iter()
        .find(|(n, _)| *n == job.options.engine)
        .map(|(_, d)| *d)
        .unwrap_or("");
    let m = json!({
        "name": job.name,
        "job_id": job.id,
        "generator": if job.mock { "mock" } else { "TRELLIS.2" },
        "app_version": crate::APP_VERSION,
        "options": job.options,
        "files": job.outputs,
        "seed_candidates": job.candidates,
        "units": "meters (glTF)",
        "up_axis": "Y (glTF)",
        "engine_notes": note,
    });
    std::fs::write(job.out_dir().join("asset.json"), serde_json::to_vec_pretty(&m)?)?;
    Ok(())
}
