//! Safe edits of the *user* PATH (HKCU\Environment\Path), used by the installer.
//! Safety rules, learned the hard way: never rewrite a value we failed to read, keep the
//! registry value type (REG_EXPAND_SZ keeps `%VARS%` working), and back the old value up first.
use anyhow::{anyhow, Result};

#[cfg(windows)]
pub fn edit(dir: &str, add: bool) -> Result<String> {
    use winreg::enums::*;
    use winreg::{RegKey, RegValue};

    let env = RegKey::predef(HKEY_CURRENT_USER).open_subkey_with_flags("Environment", KEY_READ | KEY_WRITE)?;
    let raw = match env.get_raw_value("Path") {
        Ok(v) => v,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => RegValue { bytes: vec![], vtype: REG_EXPAND_SZ },
        Err(e) => return Err(anyhow!("cannot read user PATH, leaving it untouched: {e}")),
    };
    if raw.vtype != REG_SZ && raw.vtype != REG_EXPAND_SZ {
        return Err(anyhow!("user PATH has unexpected registry type, leaving it untouched"));
    }
    let units: Vec<u16> = raw.bytes.chunks_exact(2).map(|c| u16::from_le_bytes([c[0], c[1]])).collect();
    let current = String::from_utf16(&units)?.trim_end_matches('\0').to_string();

    let norm = |s: &str| s.trim().trim_end_matches('\\').to_lowercase();
    let parts: Vec<&str> = current.split(';').filter(|s| !s.trim().is_empty()).collect();
    let present = parts.iter().any(|p| norm(p) == norm(dir));
    let updated = match (add, present) {
        (true, true) | (false, false) => return Ok("unchanged".into()),
        (true, false) => parts.iter().copied().chain(std::iter::once(dir)).collect::<Vec<_>>().join(";"),
        (false, true) => parts.iter().copied().filter(|p| norm(p) != norm(dir)).collect::<Vec<_>>().join(";"),
    };

    let backup = crate::paths::home().join("user-path-backup.txt");
    std::fs::write(&backup, &current)?;
    let mut bytes: Vec<u8> = updated.encode_utf16().flat_map(|u| u.to_le_bytes()).collect();
    bytes.extend_from_slice(&[0, 0]);
    env.set_raw_value("Path", &RegValue { bytes, vtype: raw.vtype })?;
    Ok(if add { "added" } else { "removed" }.into())
}

#[cfg(not(windows))]
pub fn edit(_dir: &str, _add: bool) -> Result<String> {
    Err(anyhow!("PATH editing is only implemented for Windows"))
}
