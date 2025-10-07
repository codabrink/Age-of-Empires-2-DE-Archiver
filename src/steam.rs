use crate::Context;

use anyhow::Result;
use anyhow::bail;
use std::path::PathBuf;
use winreg::RegKey;
use winreg::enums::*;

pub fn steam_aoe2_path(ctx: &Context) -> Result<PathBuf> {
    Ok(steam_common_path()?.join(&ctx.config.aoe2.steam_folder))
}

pub fn steam_common_path() -> Result<PathBuf> {
    Ok(steamapps_path()?.join("common"))
}
pub fn steamapps_path() -> Result<PathBuf> {
    Ok(steam_path()?.join("steamapps"))
}

pub fn steam_path() -> Result<PathBuf> {
    let hklm = RegKey::predef(HKEY_LOCAL_MACHINE);

    // Try the most common location first (64-bit systems)
    let registry_paths = [
        "SOFTWARE\\WOW6432Node\\Valve\\Steam",
        "SOFTWARE\\Valve\\Steam",
    ];

    for registry_path in &registry_paths {
        if let Ok(key) = hklm.open_subkey(registry_path) {
            if let Ok(install_path) = key.get_value::<String, _>("InstallPath") {
                return Ok(PathBuf::from(install_path));
            }
        }
    }

    // Try HKEY_CURRENT_USER as fallback
    let hkcu = RegKey::predef(HKEY_CURRENT_USER);
    if let Ok(key) = hkcu.open_subkey("SOFTWARE\\Valve\\Steam") {
        if let Ok(steam_path) = key.get_value::<String, _>("SteamPath") {
            return Ok(PathBuf::from(steam_path));
        }
    }

    bail!("Steam installation not found in registry")
}
