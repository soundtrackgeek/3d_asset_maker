use crate::options::GenOptions;
use crate::paths;
use anyhow::{anyhow, Context, Result};
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

#[derive(Serialize, Deserialize, Clone, Copy, Debug, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum Status {
    Queued,
    Running,
    Done,
    Failed,
    Cancelled,
}

#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct Job {
    pub id: String,
    pub name: String,
    pub created: u64,
    pub status: Status,
    /// Input image file name inside the job directory.
    pub input: String,
    pub options: GenOptions,
    pub mock: bool,
    pub progress: f32,
    pub stage: String,
    pub error: Option<String>,
    /// Output file names inside `<job>/out`.
    pub outputs: Vec<String>,
    pub elapsed_secs: f32,
    /// Peak GPU memory reserved by the worker (MiB), reported after sampling.
    #[serde(default)]
    pub peak_vram_mb: Option<u32>,
}

pub fn now() -> u64 {
    SystemTime::now().duration_since(UNIX_EPOCH).map(|d| d.as_secs()).unwrap_or(0)
}

impl Job {
    pub fn dir(&self) -> PathBuf {
        paths::jobs_dir().join(&self.id)
    }
    pub fn input_path(&self) -> PathBuf {
        self.dir().join(&self.input)
    }
    pub fn out_dir(&self) -> PathBuf {
        self.dir().join("out")
    }
    pub fn save(&self) -> Result<()> {
        let f = self.dir().join("job.json");
        std::fs::write(&f, serde_json::to_vec_pretty(self)?).with_context(|| format!("write {f:?}"))
    }
    pub fn output_paths(&self) -> Vec<PathBuf> {
        self.outputs.iter().map(|o| self.out_dir().join(o)).collect()
    }
}

fn new_id() -> String {
    format!("{:x}{:04x}", now(), rand::random::<u16>())
}

/// Copy an image into a new job directory and persist the job as `Queued`.
pub fn create(src: &Path, options: GenOptions, mock: bool) -> Result<Job> {
    crate::options::validate(&options).map_err(|e| anyhow!(e))?;
    let ext = src.extension().and_then(|e| e.to_str()).unwrap_or("png").to_lowercase();
    if !["png", "jpg", "jpeg", "webp"].contains(&ext.as_str()) {
        return Err(anyhow!("unsupported image type .{ext} (png, jpg, webp)"));
    }
    let id = new_id();
    let dir = paths::jobs_dir().join(&id);
    std::fs::create_dir_all(dir.join("out"))?;
    let input = format!("input.{ext}");
    std::fs::copy(src, dir.join(&input)).with_context(|| format!("read {src:?}"))?;
    let name = src.file_stem().and_then(|s| s.to_str()).unwrap_or("asset").to_string();
    let mut options = options;
    if options.randomize_seed {
        options.seed = rand::random::<u32>() as u64;
    }
    let job = Job {
        id,
        name,
        created: now(),
        status: Status::Queued,
        input,
        options,
        mock,
        progress: 0.0,
        stage: "queued".into(),
        error: None,
        outputs: vec![],
        elapsed_secs: 0.0,
        peak_vram_mb: None,
    };
    job.save()?;
    Ok(job)
}

pub fn load(id: &str) -> Result<Job> {
    let f = paths::jobs_dir().join(id).join("job.json");
    let s = std::fs::read_to_string(&f).with_context(|| format!("job '{id}' not found"))?;
    Ok(serde_json::from_str(&s)?)
}

pub fn list() -> Vec<Job> {
    let mut v: Vec<Job> = std::fs::read_dir(paths::jobs_dir())
        .into_iter()
        .flatten()
        .flatten()
        .filter_map(|e| load(&e.file_name().to_string_lossy()).ok())
        .collect();
    v.sort_by(|a, b| b.created.cmp(&a.created));
    v
}

pub fn delete(id: &str) -> Result<()> {
    if id.is_empty() || id.contains(['/', '\\', '.']) {
        return Err(anyhow!("bad job id"));
    }
    std::fs::remove_dir_all(paths::jobs_dir().join(id))?;
    Ok(())
}

/// Jobs left running by a crash are marked failed on startup.
pub fn recover_stale() {
    for mut j in list() {
        if j.status == Status::Running {
            j.status = Status::Failed;
            j.error = Some("interrupted (app closed while running)".into());
            let _ = j.save();
        }
    }
}
