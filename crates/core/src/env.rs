use std::path::PathBuf;

/// Candidate `.env.local` files, most specific first.
fn candidates() -> Vec<PathBuf> {
    let mut v = vec![];
    if let Ok(d) = std::env::current_dir() {
        v.push(d.join(".env.local"));
    }
    if let Ok(exe) = std::env::current_exe() {
        if let Some(d) = exe.parent() {
            v.push(d.join(".env.local"));
        }
    }
    v.push(crate::paths::home().join(".env.local"));
    v
}

fn parse_line(line: &str) -> Option<(String, String)> {
    let line = line.trim();
    if line.is_empty() || line.starts_with('#') {
        return None;
    }
    let (k, v) = line.split_once('=')?;
    let v = v.trim().trim_matches('"').trim_matches('\'');
    Some((k.trim().trim_start_matches("export ").to_string(), v.to_string()))
}

/// Load `.env.local` values into the process env (never overriding real env vars).
pub fn load() {
    for f in candidates() {
        if let Ok(s) = std::fs::read_to_string(&f) {
            for l in s.lines() {
                if let Some((k, v)) = parse_line(l) {
                    if std::env::var_os(&k).is_none() {
                        std::env::set_var(k, v);
                    }
                }
            }
        }
    }
}

/// Returns the Hugging Face token, if configured. Never log the value.
pub fn hf_token() -> Option<String> {
    load();
    std::env::var("HF_TOKEN").ok().filter(|s| !s.is_empty())
}
