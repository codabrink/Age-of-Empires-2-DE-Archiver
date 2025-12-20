use crate::{Context, utils::extract_7z};
use anyhow::Result;
use std::{
    collections::HashMap,
    path::{Path, PathBuf},
    sync::{Arc, LazyLock},
};
use tracing::error;
use tracing::info;

const FILES: &[&str] = &[
    "steamclient.dll",
    "steamclient64.dll",
    "coldclientloader.ini",
    "steamclient_loader_x64.exe",
];
const SUBDIRS: &[&str] = &["dlls", "steam_settings", "saves"];

const STEAM_SETTINGS_FILES_SLICE: &[(&str, &str)] = &[
    (
        "supported_languages.txt",
        include_str!("../assets/supported_languages.txt"),
    ),
    (
        "achievements.json",
        include_str!("../assets/achievements.json"),
    ),
    ("configs.app.ini", include_str!("../assets/configs.app.ini")),
    (
        "configs.user.ini",
        include_str!("../assets/configs.user.ini"),
    ),
];
static STEAM_SETTINGS_FILES: LazyLock<HashMap<String, String>> = LazyLock::new(|| {
    STEAM_SETTINGS_FILES_SLICE
        .iter()
        .map(|(name, content)| (name.to_string(), content.to_string()))
        .collect()
});

pub fn spawn_apply(ctx: Arc<Context>) -> Result<()> {
    let busy = ctx.busy.lock()?;
    std::thread::spawn(move || {
        let _busy = busy;
        ctx.set_step_status(1, crate::StepStatus::InProgress);
        match apply_goldberg(ctx.clone()) {
            Ok(_) => {
                ctx.set_step_status(1, crate::StepStatus::Completed);
                ctx.working_on("Goldberg emulator applied successfully");
            }
            Err(err) => {
                let err_msg = format!("{:#}", err);
                ctx.set_step_status(1, crate::StepStatus::Failed(err_msg.clone()));
                ctx.send_error(format!("Goldberg installation failed: {}", err_msg));
                error!("{err:?}");
            }
        }
    });
    Ok(())
}

pub fn apply_goldberg(ctx: Arc<Context>) -> Result<()> {
    ctx.working_on("Downloading Goldberg Emulator");

    let goldberg_archive = {
        let dl_url = &ctx.config.goldberg.download_url;
        info!("Downloading goldberg. {dl_url}",);
        let gbe_archive = reqwest::blocking::get(dl_url)?.bytes()?.to_vec();

        ctx.working_on("Extracting Goldberg Emulator Archive".to_string());
        extract_7z(&gbe_archive)?
    };

    let output_dir = ctx.outdir()?;

    ctx.working_on("Patching goldberg into export.");
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
    ctx.working_on("Patching goldberg configs");
    update_cold_client_loader(&output_dir.join("ColdClientLoader.ini"))?;

    for (filename, default_file) in &*STEAM_SETTINGS_FILES {
        let src_path = PathBuf::from("assets").join(filename);
        let dest_path = output_dir.join("steam_settings").join(filename);
        if std::fs::exists(&src_path)? {
            std::fs::copy(src_path, dest_path)?;
        } else {
            std::fs::write(dest_path, default_file)?;
        }
    }

    ctx.working_on("Done installing goldberg.");

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

#[allow(dead_code)]
pub fn latest_release(ctx: &Context) -> Result<HashMap<String, Vec<u8>>> {
    let archive = reqwest::blocking::get(&ctx.config.goldberg.download_url)?.bytes()?;
    extract_7z(&archive.to_vec())
}
