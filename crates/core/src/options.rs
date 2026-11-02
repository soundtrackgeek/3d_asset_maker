use serde::{Deserialize, Serialize};

#[derive(Serialize, Deserialize, Clone, Debug, Default)]
#[serde(default)]
pub struct Sampler {
    pub steps: Option<u32>,
    pub guidance_strength: Option<f32>,
    pub guidance_rescale: Option<f32>,
    pub rescale_t: Option<f32>,
}

#[derive(Serialize, Deserialize, Clone, Debug)]
#[serde(default)]
pub struct GenOptions {
    /// draft | balanced | high (informational once expanded)
    pub preset: String,
    /// TRELLIS.2 pipeline type: 512 | 1024 | 1024_cascade | 1536_cascade
    pub resolution: String,
    pub seed: u64,
    pub randomize_seed: bool,
    /// Remove background before generation (needs gated RMBG model -> HF_TOKEN).
    pub preprocess_image: bool,
    pub max_num_tokens: u32,
    /// Offload sub-models to CPU between stages (essential on 16 GB).
    pub low_vram: bool,
    /// sdpa | flash_attn | xformers
    pub attn_backend: String,
    pub sparse_structure: Sampler,
    pub shape: Sampler,
    pub texture: Sampler,
    /// Target triangle count after decimation.
    pub decimation_target: u32,
    pub texture_size: u32,
    pub remesh: bool,
    /// WebP textures are smaller but not supported by every engine importer.
    pub webp_textures: bool,
    /// glb, obj, ply
    pub formats: Vec<String>,
    /// none | threejs | unity | unreal | godot
    pub engine: String,
    pub model_id: String,
}

impl Default for GenOptions {
    fn default() -> Self {
        preset("balanced").unwrap()
    }
}

pub const PRESETS: &[(&str, &str)] = &[
    ("draft", "512 pipeline, 1K textures. Fast sanity check."),
    ("balanced", "1024 cascade, 2K textures. Recommended for 16 GB cards."),
    ("high", "1536 cascade, 4K textures. Experimental on 16 GB; may run out of VRAM."),
];

pub fn preset(name: &str) -> Option<GenOptions> {
    let (res, tex, dec) = match name {
        "draft" => ("512", 1024, 50_000),
        "balanced" => ("1024_cascade", 2048, 150_000),
        "high" => ("1536_cascade", 4096, 500_000),
        _ => return None,
    };
    Some(GenOptions {
        preset: name.into(),
        resolution: res.into(),
        seed: 42,
        randomize_seed: false,
        preprocess_image: true,
        max_num_tokens: if name == "high" { 49152 } else { 32768 },
        low_vram: true,
        attn_backend: "sdpa".into(),
        sparse_structure: Sampler::default(),
        shape: Sampler::default(),
        texture: Sampler::default(),
        decimation_target: dec,
        texture_size: tex,
        remesh: true,
        webp_textures: false,
        formats: vec!["glb".into()],
        engine: "none".into(),
        model_id: "microsoft/TRELLIS.2-4B".into(),
    })
}

pub const ENGINES: &[(&str, &str)] = &[
    ("none", "No engine tuning; keep preset values."),
    ("threejs", "Web: max 150k tris, 2K textures. Load with GLTFLoader."),
    ("unity", "Unity: max 100k tris, 2K textures. Import GLB via the glTFast package."),
    ("unreal", "Unreal Engine 5: max 200k tris, 4K textures. Import GLB via the Interchange pipeline."),
    ("godot", "Godot 4: max 100k tris, 2K textures. Drop the GLB into res:// and it imports natively."),
];

/// Cap an options set to an engine's sensible budget.
pub fn apply_engine(o: &mut GenOptions, engine: &str) {
    let (dec, tex) = match engine {
        "threejs" => (150_000, 2048),
        "unity" | "godot" => (100_000, 2048),
        "unreal" => (200_000, 4096),
        _ => return,
    };
    o.engine = engine.into();
    o.decimation_target = o.decimation_target.min(dec);
    o.texture_size = o.texture_size.min(tex);
}

pub fn validate(o: &GenOptions) -> Result<(), String> {
    if !["512", "1024", "1024_cascade", "1536_cascade"].contains(&o.resolution.as_str()) {
        return Err(format!("invalid resolution '{}'", o.resolution));
    }
    if !o.texture_size.is_power_of_two() || o.texture_size < 256 || o.texture_size > 8192 {
        return Err("texture_size must be a power of two between 256 and 8192".into());
    }
    for f in &o.formats {
        if !["glb", "obj", "ply"].contains(&f.as_str()) {
            return Err(format!("unsupported format '{f}' (use glb, obj, ply)"));
        }
    }
    if o.formats.is_empty() {
        return Err("at least one output format required".into());
    }
    Ok(())
}
