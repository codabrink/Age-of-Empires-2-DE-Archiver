use crate::{
    Context,
    utils::{extract_zip, gh_latest_release_dl_url},
};
use anyhow::{Result, bail};
use std::fs::{self, read_to_string};

pub fn install_launcher(ctx: Context) -> Result<()> {
    let Some(launcher_url) = launcher_full_url(&ctx)? else {
        bail!("Unable to find latest launcher release.");
    };
    ctx.working_on("Downloading launcher.");

    let launcher_zip = reqwest::blocking::get(launcher_url)?.bytes()?.to_vec();
    let outdir = ctx.outdir()?;

    ctx.working_on("Extracting launcher.");

    for (name, file) in extract_zip(&launcher_zip)? {
        let mut outpath = outdir.clone();
        name.split("/").for_each(|c| outpath = outpath.join(c));

        if let Some(parent) = outpath.parent() {
            dbg!("parent", parent);
            if !parent.exists() {
                fs::create_dir_all(parent)?;
            }
        }
        fs::write(outpath, file)?;
    }

    patch_launcher_config(&ctx)?;

    Ok(())
}

fn patch_launcher_config(ctx: &Context) -> Result<()> {
    // Set the executable directory.
    let outdir = ctx.outdir()?;
    ctx.working_on("Patching launcher config.");
    let aoe2_config_path = outdir
        .join("launcher")
        .join("resources")
        .join("config.aoe2.toml");
    let aoe2_config = read_to_string(&aoe2_config_path)?;
    let aoe2_config = aoe2_config.replace(
        "Executable = 'auto'",
        "Executable = '.\\..\\steamclient_loader_x64.exe'",
    );
    let aoe2_config = aoe2_config.replace("Path = 'auto'", "Path = '.\\..\\AoE2DE'");
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
