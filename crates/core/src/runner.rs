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
        .env("PYTHONIOENCODING", "utf-8")
        .env("PYTORCH_CUDA_ALLOC_CONF", "expandable_segments:True")
        .env("ATTN_BACKEND", &job.options.attn_backend)
        .env("PYTHONPATH", &repo)
        .stdout(Stdio::piped())
        .stderr(Stdio::from(log));
    if let Some(t) = env::hf_token() {
        cmd.env("HF_TOKEN", &t).env("HUGGING_FACE_HUB_TOKEN", t);
    }
    no_window(&mut cmd);
    let mut child = cmd.spawn().map_err(|e| anyhow!("failed to start python worker: {e}"))?;
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
        "units": "meters (glTF)",
        "up_axis": "Y (glTF)",
        "engine_notes": note,
    });
    std::fs::write(job.out_dir().join("asset.json"), serde_json::to_vec_pretty(&m)?)?;
    Ok(())
}
