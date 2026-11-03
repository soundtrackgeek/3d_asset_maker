//! `t2` — headless, agent-first CLI for TRELLIS.2 Studio.
//! Contract: stdout is always a single JSON document; progress/logs go to stderr.
//! Exit codes: 0 ok, 1 usage/runtime error, 2 a generation job failed.
use anyhow::{anyhow, Result};
use clap::{Args, Parser, Subcommand};
use serde_json::{json, Value};
use std::path::PathBuf;
use std::sync::atomic::AtomicBool;
use trellis_core::{doctor, env, job, options, runner, setup, update};

#[derive(Parser)]
#[command(name = "t2", version, about = "TRELLIS.2 Studio CLI: image -> game-ready 3D asset (agent-first, JSON on stdout)")]
struct Cli {
    #[command(subcommand)]
    cmd: Cmd,
}

#[derive(Subcommand)]
enum Cmd {
    /// Check GPU, runtime, HF token; recommends a preset.
    Doctor,
    /// Install the Python/TRELLIS.2 runtime (Windows, NVIDIA).
    Setup,
    /// List presets, engine targets and the full options schema (defaults).
    Options,
    /// Generate 3D assets from one or more images.
    Generate(GenArgs),
    /// Manage past jobs.
    Jobs {
        #[command(subcommand)]
        cmd: JobsCmd,
    },
    /// Add/remove a folder on the user PATH (used by the installer; backs up the old value).
    Path {
        #[command(subcommand)]
        cmd: PathCmd,
    },
    /// Check GitHub Releases for a newer version.
    Update {
        #[command(subcommand)]
        cmd: UpdateCmd,
    },
}

#[derive(Subcommand)]
enum JobsCmd {
    List,
    Show { id: String },
    Delete { id: String },
}

#[derive(Subcommand)]
enum PathCmd {
    Add { dir: String },
    Remove { dir: String },
}

#[derive(Subcommand)]
enum UpdateCmd {
    Check,
    /// Download the latest installer and run it silently (the CLI exits first on Windows installs).
    Install,
}

#[derive(Args)]
struct GenArgs {
    /// Input image(s): png, jpg, webp.
    #[arg(required = true)]
    images: Vec<PathBuf>,
    /// draft | balanced | high
    #[arg(long, default_value = "draft")]
    preset: String,
    /// none | threejs | unity | unreal | godot (caps triangle/texture budget)
    #[arg(long)]
    engine: Option<String>,
    /// 512 | 1024 | 1024_cascade | 1536_cascade
    #[arg(long)]
    resolution: Option<String>,
    #[arg(long)]
    seed: Option<u64>,
    #[arg(long)]
    randomize_seed: bool,
    /// Target triangle count.
    #[arg(long)]
    decimate: Option<u32>,
    /// Texture size (power of two).
    #[arg(long)]
    texture_size: Option<u32>,
    /// Comma separated: glb,obj,ply
    #[arg(long, value_delimiter = ',')]
    format: Option<Vec<String>>,
    /// Skip background removal (use when the input already has a clean alpha/solid background).
    #[arg(long)]
    no_bg_removal: bool,
    /// Disable low-VRAM offloading (faster on 24 GB+ cards).
    #[arg(long)]
    no_low_vram: bool,
    /// JSON file with option overrides (same shape as `t2 options`).
    #[arg(long)]
    options_file: Option<PathBuf>,
    /// GPU-free dry run: writes a placeholder cube GLB.
    #[arg(long)]
    mock: bool,
    /// Copy results into this directory as <out>/<name>/...
    #[arg(long)]
    out: Option<PathBuf>,
    /// Suppress progress lines on stderr.
    #[arg(long, short)]
    quiet: bool,
}

fn build_options(a: &GenArgs) -> Result<options::GenOptions> {
    let mut base = options::preset(&a.preset).ok_or_else(|| anyhow!("unknown preset '{}' (draft|balanced|high)", a.preset))?;
    if let Some(f) = &a.options_file {
        let mut v = serde_json::to_value(&base)?;
        let over: Value = serde_json::from_str(&std::fs::read_to_string(f)?)?;
        if let (Some(m), Some(o)) = (v.as_object_mut(), over.as_object()) {
            for (k, x) in o {
                m.insert(k.clone(), x.clone());
            }
        }
        base = serde_json::from_value(v)?;
    }
    if let Some(r) = &a.resolution {
        base.resolution = r.clone();
    }
    if let Some(s) = a.seed {
        base.seed = s;
    }
    base.randomize_seed |= a.randomize_seed;
    if let Some(d) = a.decimate {
        base.decimation_target = d;
    }
    if let Some(t) = a.texture_size {
        base.texture_size = t;
    }
    if let Some(f) = &a.format {
        base.formats = f.clone();
    }
    if a.no_bg_removal {
        base.preprocess_image = false;
    }
    if a.no_low_vram {
        base.low_vram = false;
    }
    if let Some(e) = &a.engine {
        if !options::ENGINES.iter().any(|(n, _)| n == e) {
            return Err(anyhow!("unknown engine '{e}' (none|threejs|unity|unreal|godot)"));
        }
        options::apply_engine(&mut base, e);
    }
    options::validate(&base).map_err(|e| anyhow!(e))?;
    Ok(base)
}

fn generate(a: GenArgs) -> Result<(Value, bool)> {
    let opts = build_options(&a)?;
    let cancel = AtomicBool::new(false);
    let mut results = vec![];
    let mut any_failed = false;
    for img in &a.images {
        let mut j = job::create(img, opts.clone(), a.mock)?;
        let quiet = a.quiet;
        let name = j.name.clone();
        let mut last = String::new();
        let res = runner::run(&mut j, &cancel, &mut |j| {
            if !quiet && j.stage != last {
                eprintln!("[{name}] {:>3.0}% {}", j.progress * 100.0, j.stage);
                last = j.stage.clone();
            }
        });
        let mut files: Vec<String> = j.output_paths().iter().map(|p| p.display().to_string()).collect();
        if res.is_ok() {
            if let Some(out) = &a.out {
                let dest = out.join(&j.name);
                std::fs::create_dir_all(&dest)?;
                files.clear();
                for f in j.outputs.iter().chain(std::iter::once(&"asset.json".to_string())) {
                    let to = dest.join(f);
                    std::fs::copy(j.out_dir().join(f), &to)?;
                    files.push(to.display().to_string());
                }
            }
        } else {
            any_failed = true;
        }
        results.push(json!({
            "image": img, "job_id": j.id, "status": j.status, "elapsed_secs": j.elapsed_secs,
            "files": files, "error": j.error, "seed": j.options.seed,
        }));
    }
    Ok((json!({ "results": results }), any_failed))
}

fn run() -> Result<(Value, i32)> {
    env::load();
    let cli = Cli::parse();
    Ok(match cli.cmd {
        Cmd::Doctor => (serde_json::to_value(doctor::run())?, 0),
        Cmd::Setup => {
            setup::run_setup(&mut |l| eprintln!("{l}"))?;
            (json!({"ok": true, "next": "run `t2 doctor`"}), 0)
        }
        Cmd::Options => (
            json!({
                "presets": options::PRESETS.iter().map(|(n, d)| json!({"name": n, "description": d})).collect::<Vec<_>>(),
                "engines": options::ENGINES.iter().map(|(n, d)| json!({"name": n, "description": d})).collect::<Vec<_>>(),
                "resolutions": ["512", "1024", "1024_cascade", "1536_cascade"],
                "formats": ["glb", "obj", "ply"],
                "defaults": options::GenOptions::default(),
                "home": trellis_core::paths::home(),
            }),
            0,
        ),
        Cmd::Generate(a) => {
            let (v, failed) = generate(a)?;
            (v, if failed { 2 } else { 0 })
        }
        Cmd::Jobs { cmd } => match cmd {
            JobsCmd::List => (json!({"jobs": job::list()}), 0),
            JobsCmd::Show { id } => (serde_json::to_value(job::load(&id)?)?, 0),
            JobsCmd::Delete { id } => {
                job::delete(&id)?;
                (json!({"deleted": id}), 0)
            }
        },
        Cmd::Path { cmd } => {
            let (dir, add) = match cmd {
                PathCmd::Add { dir } => (dir, true),
                PathCmd::Remove { dir } => (dir, false),
            };
            (json!({"result": trellis_core::winpath::edit(&dir, add)?, "dir": dir}), 0)
        }
        Cmd::Update { cmd: UpdateCmd::Check } => (serde_json::to_value(update::check()?)?, 0),
        Cmd::Update { cmd: UpdateCmd::Install } => {
            let u = update::check()?;
            match (u.available, &u.download_url) {
                (true, Some(url)) => {
                    let f = update::download_and_run_installer(url, true)?;
                    (json!({"installing": u.latest, "installer": f}), 0)
                }
                (true, None) => return Err(anyhow!("release {} has no installer asset", u.latest)),
                _ => (json!({"installing": null, "message": "already up to date", "current": u.current}), 0),
            }
        }
    })
}

fn main() {
    match run() {
        Ok((v, code)) => {
            println!("{}", serde_json::to_string_pretty(&v).unwrap());
            std::process::exit(code);
        }
        Err(e) => {
            println!("{}", json!({"error": format!("{e:#}")}));
            std::process::exit(1);
        }
    }
}
