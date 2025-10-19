use anyhow::Result;
use anyhow::bail;
use std::path::PathBuf;
use winreg::RegKey;
use winreg::enums::*;

pub fn steam_aoe2_path() -> Result<PathBuf> {
    install_location("Steam App 813780")
}

pub fn install_location(app_name: &str) -> Result<PathBuf> {
    let hklm = RegKey::predef(HKEY_LOCAL_MACHINE);

    // Try the most common location first (64-bit systems)
    const ROOT: &str = "SOFTWARE\\Microsoft\\Windows\\CurrentVersion\\Uninstall\\";
    let mut registry_path = ROOT.to_string();
    registry_path.push_str(app_name);

    if let Ok(key) = hklm.open_subkey(registry_path) {
        if let Ok(install_path) = key.get_value::<String, _>("InstallLocation") {
            return Ok(PathBuf::from(install_path));
        }
    }

    bail!("Unable to find install location in registry")
}
