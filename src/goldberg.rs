use crate::{
    AppState, Context,
    utils::{desktop_dir, extract_7z},
};
use anyhow::Result;
use std::{collections::HashMap, path::Path};

const FILES: &[&str] = &[
    "steamclient.dll",
    "steamclient64.dll",
    "coldclientloader.ini",
    "steamclient_loader_x64.exe",
];
const SUBDIRS: &[&str] = &["dlls", "steam_settings", "saves"];
const STEAM_SETTINGS_FILES: &[&str] = &[
    "supported_languages.txt",
    "achievements.json",
    "configs.app.ini",
    "configs.user.ini",
];

pub fn apply(ctx: &Context) -> Result<()> {
    ctx.tx.send(AppState::Working(
        "Downloading Goldberg Emulator".to_string(),
    ))?;

    let goldberg_archive = {
        let gbe_archive = reqwest::blocking::get(&ctx.config.goldberg.download_url)?
            .bytes()?
            .to_vec();
        ctx.tx.send(AppState::Working(
            "Extracting Goldberg Emulator Archive".to_string(),
        ))?;
        extract_7z(&gbe_archive)?
    };

    let output_dir = desktop_dir()?.join("AoE2");

    for (path, file) in goldberg_archive {
        const EXPERIMENTAL: &str = "release/steamclient_experimental/";
        if !path.starts_with(EXPERIMENTAL) {
            continue;
        }
        let path = path.replace(EXPERIMENTAL, "");

        if !FILES.contains(&&*path.to_lowercase()) {
            continue;
        }

        let path = output_dir.join(path.replace(EXPERIMENTAL, ""));

        if let Some(parent) = path.parent() {
            if !parent.exists() {
                let _ = std::fs::create_dir_all(parent);
            }
        }

        let _ = std::fs::write(&path, file);
    }

    for subdir in SUBDIRS {
        let _ = std::fs::create_dir_all(output_dir.join(subdir));
    }

    // Configure goldberg for AoE2
    update_cold_client_loader(&output_dir.join("ColdClientLoader.ini"))?;

    for settings_file in STEAM_SETTINGS_FILES {
        std::fs::copy(
            Path::new("assets").join(settings_file),
            output_dir.join("steam_settings").join(settings_file),
        )?;
    }

    Ok(())
}

fn update_cold_client_loader(ini_path: &Path) -> Result<()> {
    use ini::Ini;

    let mut conf = Ini::load_from_file(ini_path)?;

    conf.with_section(Some("SteamClient"))
        .set(
            "Exe",
            Path::new("Aoe2DE").join("AoE2DE_s.exe").to_string_lossy(),
        )
        .set("AppId", "813780");
    conf.with_section(Some("Injection"))
        .set("DllsToInjectFolder", "dlls");

    conf.write_to_file(ini_path)?;

    Ok(())
}

pub fn latest_release(ctx: &Context) -> Result<HashMap<String, Vec<u8>>> {
    let archive = reqwest::blocking::get(&ctx.config.goldberg.download_url)?.bytes()?;
    extract_7z(&archive.to_vec())
}
