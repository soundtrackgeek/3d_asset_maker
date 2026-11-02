use anyhow::{anyhow, Result};
use serde::Serialize;

#[derive(Serialize, Clone, Debug)]
pub struct UpdateInfo {
    pub current: String,
    pub latest: String,
    pub available: bool,
    pub release_url: String,
    /// Direct download of the installer (`*-Setup.exe`), if the release has one.
    pub download_url: Option<String>,
    pub notes: String,
}

fn parse(v: &str) -> (u64, u64, u64) {
    let mut it = v.trim().trim_start_matches('v').split(['.', '-', '+']).map(|p| p.parse::<u64>().unwrap_or(0));
    (it.next().unwrap_or(0), it.next().unwrap_or(0), it.next().unwrap_or(0))
}

pub fn is_newer(latest: &str, current: &str) -> bool {
    parse(latest) > parse(current)
}

/// Query GitHub Releases for the latest published version (blocking).
pub fn check() -> Result<UpdateInfo> {
    let url = format!("https://api.github.com/repos/{}/releases/latest", crate::REPO);
    let current = crate::APP_VERSION.to_string();
    let resp = ureq::get(&url)
        .set("User-Agent", "trellis-studio-updater")
        .set("Accept", "application/vnd.github+json")
        .timeout(std::time::Duration::from_secs(15))
        .call();
    let v: serde_json::Value = match resp {
        Ok(r) => r.into_json()?,
        Err(ureq::Error::Status(404, _)) => {
            return Ok(UpdateInfo {
                latest: current.clone(),
                current,
                available: false,
                release_url: format!("https://github.com/{}/releases", crate::REPO),
                download_url: None,
                notes: "no releases published yet".into(),
            })
        }
        Err(e) => return Err(anyhow!("update check failed: {e}")),
    };
    let tag = v["tag_name"].as_str().ok_or_else(|| anyhow!("release has no tag_name"))?.to_string();
    let download_url = v["assets"].as_array().and_then(|a| {
        a.iter()
            .find(|x| x["name"].as_str().is_some_and(|n| n.to_lowercase().ends_with("-setup.exe")))
            .and_then(|x| x["browser_download_url"].as_str().map(String::from))
    });
    Ok(UpdateInfo {
        available: is_newer(&tag, &current),
        latest: tag.trim_start_matches('v').to_string(),
        current,
        release_url: v["html_url"].as_str().unwrap_or_default().to_string(),
        download_url,
        notes: v["body"].as_str().unwrap_or_default().to_string(),
    })
}

/// Download the installer from this project's GitHub release and launch it.
/// `silent` passes the NSIS `/S` flag (used by the CLI); the GUI shows the wizard.
/// The caller should exit afterwards so the installer can replace the running exe.
pub fn download_and_run_installer(url: &str, silent: bool) -> Result<std::path::PathBuf> {
    let prefix = format!("https://github.com/{}/releases/download/", crate::REPO);
    if !url.starts_with(&prefix) || !url.to_lowercase().ends_with("-setup.exe") {
        return Err(anyhow!("refusing to run installer from untrusted url"));
    }
    let dest = crate::paths::home().join("tmp");
    std::fs::create_dir_all(&dest)?;
    let file = dest.join(url.rsplit('/').next().unwrap_or("TRELLIS-Studio-Setup.exe"));
    let resp = ureq::get(url).set("User-Agent", "trellis-studio-updater").timeout(std::time::Duration::from_secs(600)).call()?;
    let mut out = std::fs::File::create(&file)?;
    std::io::copy(&mut resp.into_reader(), &mut out)?;
    drop(out);
    let mut cmd = std::process::Command::new(&file);
    if silent {
        cmd.arg("/S");
    }
    cmd.spawn().map_err(|e| anyhow!("cannot start installer: {e}"))?;
    Ok(file)
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn compares_versions() {
        assert!(is_newer("v0.2.0", "0.1.9"));
        assert!(is_newer("1.0.0", "0.9.9"));
        assert!(!is_newer("0.1.0", "0.1.0"));
        assert!(!is_newer("0.0.9", "0.1.0"));
    }
}
