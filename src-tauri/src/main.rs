#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

use base64::{engine::general_purpose::STANDARD as B64, Engine};
use serde::Deserialize;
use serde_json::{json, Value};
use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use tauri::{AppHandle, Emitter, State};
use trellis_core::job::{self, Job, Status};
use trellis_core::{doctor, env, options, paths, runner, setup, update};

#[derive(Default)]
struct AppState {
    cancels: Mutex<HashMap<String, Arc<AtomicBool>>>,
    /// One generation at a time: the GPU (16 GB) cannot hold two pipelines.
    gpu: Arc<Mutex<()>>,
}

type Res<T> = Result<T, String>;
fn err<E: std::fmt::Display>(e: E) -> String {
    format!("{e:#}")
}

#[tauri::command]
fn app_info() -> Value {
    json!({
        "version": trellis_core::APP_VERSION,
        "home": paths::home(),
        "hf_token": env::hf_token().is_some(),
        "repo": trellis_core::REPO,
    })
}

#[tauri::command]
fn get_options() -> Value {
    json!({
        "presets": options::PRESETS.iter().map(|(n, d)| json!({"name": n, "description": d, "options": options::preset(n)})).collect::<Vec<_>>(),
        "engines": options::ENGINES.iter().map(|(n, d)| json!({"name": n, "description": d})).collect::<Vec<_>>(),
    })
}

#[tauri::command]
async fn run_doctor() -> Res<Value> {
    tauri::async_runtime::spawn_blocking(|| serde_json::to_value(doctor::run()).map_err(err))
        .await
        .map_err(err)?
}

#[tauri::command]
fn list_jobs() -> Vec<Job> {
    job::list()
}

fn apply_engine_opts(mut o: options::GenOptions) -> options::GenOptions {
    let e = o.engine.clone();
    options::apply_engine(&mut o, &e);
    o
}

#[derive(Deserialize)]
struct Upload {
    name: String,
    data_b64: String,
}

#[tauri::command]
fn create_jobs_from_bytes(files: Vec<Upload>, opts: options::GenOptions, mock: bool) -> Res<Vec<Job>> {
    let tmp = paths::home().join("tmp");
    std::fs::create_dir_all(&tmp).map_err(err)?;
    let opts = apply_engine_opts(opts);
    let mut out = vec![];
    for f in files {
        let name = PathBuf::from(&f.name).file_name().map(PathBuf::from).ok_or("bad file name")?;
        let p = tmp.join(name);
        std::fs::write(&p, B64.decode(&f.data_b64).map_err(err)?).map_err(err)?;
        let j = job::create(&p, opts.clone(), mock).map_err(err)?;
        let _ = std::fs::remove_file(&p);
        out.push(j);
    }
    Ok(out)
}

#[tauri::command]
fn create_jobs_from_paths(paths_in: Vec<String>, opts: options::GenOptions, mock: bool) -> Res<Vec<Job>> {
    let opts = apply_engine_opts(opts);
    let mut out = vec![];
    for p in paths_in {
        let pb = PathBuf::from(&p);
        let ok = pb.extension().and_then(|e| e.to_str()).map(|e| matches!(e.to_lowercase().as_str(), "png" | "jpg" | "jpeg" | "webp")).unwrap_or(false);
        if ok {
            out.push(job::create(&pb, opts.clone(), mock).map_err(err)?);
        }
    }
    Ok(out)
}

#[tauri::command]
fn start_job(app: AppHandle, state: State<AppState>, id: String, opts: Option<options::GenOptions>) -> Res<()> {
    let mut j = job::load(&id).map_err(err)?;
    if j.status == Status::Running {
        return Err("job is already running".into());
    }
    if let Some(o) = opts {
        let mut o = apply_engine_opts(o);
        options::validate(&o).map_err(err)?;
        if o.randomize_seed {
            o.seed = (std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_nanos()).unwrap_or(0) % u32::MAX as u128) as u64;
        }
        j.options = o;
    }
    j.status = Status::Queued;
    j.stage = "queued".into();
    j.progress = 0.0;
    j.save().map_err(err)?;
    let _ = app.emit("job-updated", &j);

    let cancel = Arc::new(AtomicBool::new(false));
    state.cancels.lock().unwrap().insert(id.clone(), cancel.clone());
    let gpu = state.gpu.clone();
    std::thread::spawn(move || {
        let _g = gpu.lock().unwrap_or_else(|p| p.into_inner());
        if cancel.load(Ordering::Relaxed) {
            j.status = Status::Cancelled;
            j.stage = "cancelled".into();
            let _ = j.save();
            let _ = app.emit("job-updated", &j);
            return;
        }
        let a2 = app.clone();
        let _ = runner::run(&mut j, &cancel, &mut |j| {
            let _ = a2.emit("job-updated", j);
        });
    });
    Ok(())
}

#[tauri::command]
fn cancel_job(state: State<AppState>, id: String) {
    if let Some(c) = state.cancels.lock().unwrap().get(&id) {
        c.store(true, Ordering::Relaxed);
    }
}

#[tauri::command]
fn delete_job(id: String) -> Res<()> {
    job::delete(&id).map_err(err)
}

fn mime_of(p: &std::path::Path) -> &'static str {
    match p.extension().and_then(|e| e.to_str()).unwrap_or("").to_lowercase().as_str() {
        "png" => "image/png",
        "jpg" | "jpeg" => "image/jpeg",
        "webp" => "image/webp",
        "glb" => "model/gltf-binary",
        _ => "application/octet-stream",
    }
}

/// Returns `data:` URL for the job's input image or an output file (for <img>/<model-viewer>).
#[tauri::command]
fn read_asset(id: String, kind: String, file: Option<String>) -> Res<String> {
    let j = job::load(&id).map_err(err)?;
    let p = match kind.as_str() {
        "input" => j.input_path(),
        "output" => {
            let f = file.ok_or("file required")?;
            if f.contains(['/', '\\']) {
                return Err("bad file".into());
            }
            j.out_dir().join(f)
        }
        _ => return Err("bad kind".into()),
    };
    let bytes = std::fs::read(&p).map_err(err)?;
    Ok(format!("data:{};base64,{}", mime_of(&p), B64.encode(bytes)))
}

#[tauri::command]
fn reveal_job(id: String) -> Res<()> {
    let j = job::load(&id).map_err(err)?;
    std::process::Command::new("explorer").arg(j.out_dir()).spawn().map_err(err)?;
    Ok(())
}

#[tauri::command]
fn open_url(url: String) -> Res<()> {
    if !url.starts_with("https://github.com/") && !url.starts_with("https://huggingface.co/") {
        return Err("blocked url".into());
    }
    // `explorer <url>` opens the default browser without going through a shell.
    std::process::Command::new("explorer").arg(url).spawn().map_err(err)?;
    Ok(())
}

#[tauri::command]
async fn check_update() -> Res<Value> {
    tauri::async_runtime::spawn_blocking(|| update::check().map_err(err).and_then(|u| serde_json::to_value(u).map_err(err)))
        .await
        .map_err(err)?
}

#[tauri::command]
async fn install_runtime(app: AppHandle) -> Res<()> {
    tauri::async_runtime::spawn_blocking(move || {
        setup::run_setup(&mut |l| {
            let _ = app.emit("setup-line", l);
        })
        .map_err(err)
    })
    .await
    .map_err(err)?
}

fn main() {
    env::load();
    job::recover_stale();
    tauri::Builder::default()
        .manage(AppState::default())
        .invoke_handler(tauri::generate_handler![
            app_info,
            get_options,
            run_doctor,
            list_jobs,
            create_jobs_from_bytes,
            create_jobs_from_paths,
            start_job,
            cancel_job,
            delete_job,
            read_asset,
            reveal_job,
            open_url,
            check_update,
            install_runtime
        ])
        .run(tauri::generate_context!())
        .expect("error while running TRELLIS Studio");
}
