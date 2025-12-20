use crate::{
    Context,
    utils::{extract_zip, gh_latest_release_dl_url},
};
use anyhow::{Result, bail};
use std::{
    fs::{self, read_to_string},
    process::Command,
    sync::Arc,
};

pub fn spawn_install_launcher(ctx: Arc<Context>) -> Result<()> {
    let busy = ctx.busy.lock();

    std::thread::spawn(move || {
        let _busy = busy;
        install_launcher(ctx);
    });

    Ok(())
}

fn install_launcher(ctx: Arc<Context>) -> Result<()> {
    let Some(launcher_url) = launcher_full_url(&ctx)? else {
        bail!("Unable to find latest launcher release.");
    };
    ctx.working_on("Downloading launcher.");

    let launcher_zip = reqwest::blocking::get(launcher_url)?.bytes()?.to_vec();
    let outdir = ctx.outdir()?;

    ctx.working_on("Extracting launcher.");

    for (name, file) in extract_zip(&launcher_zip)? {
        let mut outpath = outdir.to_path_buf();
        name.split("/").for_each(|c| outpath = outpath.join(c));

        if let Some(parent) = outpath.parent() {
            if !parent.exists() {
                fs::create_dir_all(parent)?;
            }
        }
        fs::write(outpath, file)?;
    }

    patch_launcher_config(&ctx)?;

    ctx.working_on("Generating certs.");

    let gen_certs_exe = outdir.join("server").join("bin").join("genCert.exe");

    let _ = Command::new(gen_certs_exe).status();

    ctx.working_on("Done installing launcher.");

    Ok(())
}

fn patch_launcher_config(ctx: &Context) -> Result<()> {
    // Set the executable directory.
    let outdir = ctx.outdir()?;
    ctx.working_on("Patching launcher config.");
    let aoe2_config_path = outdir
        .join("launcher")
        .join("resources")
        .join("config.age2.toml");
    let aoe2_config = read_to_string(&aoe2_config_path)?;
    let aoe2_config = aoe2_config.replace(
        "Executable = 'auto'",
        "Executable = '.\\..\\steamclient_loader_x64.exe'",
    );
    let aoe2_config = aoe2_config.replace("Path = 'auto'", r#"Path = ".\\..\\AoE2DE""#);
    let aoe2_config = aoe2_config.replace("ExecutableArgs = []", r#"ExecutableArgs = [--overrideHosts=".\..\dlls\ageLANServerLauncherCompanion_AgeFakeHost_1.0.0.0.dll"]"#);
    fs::write(aoe2_config_path, aoe2_config.as_bytes())?;

    Ok(())
}

fn launcher_full_url(ctx: &Context) -> Result<Option<String>> {
    ctx.working_on("Getting latest launcher release url.");
    gh_latest_release_dl_url(
        &ctx.config.aoe2.gh_launcher_user,
        &ctx.config.aoe2.gh_launcher_repo,
        &["_full_", "win_x86-64"],
    )
}
