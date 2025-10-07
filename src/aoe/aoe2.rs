use std::fs;

use crate::{Context, utils::extract_zip};
use anyhow::{Result, bail};
use serde_json::Value;

pub fn install_launcher_companion(ctx: Context) -> Result<()> {
    let Some(companion_full_url) = launcher_companion_full_url(&ctx)? else {
        bail!("Unable to find latest release");
    };

    ctx.working_on("Downloading launcher companion.");

    let companion = reqwest::blocking::get(companion_full_url)?
        .bytes()?
        .to_vec();

    let outdir = ctx.outdir()?;
    ctx.working_on("Extracting launcher companion dlls.");
    for (name, file) in extract_zip(&companion)? {
        let lc_name = name.to_lowercase();
        if !lc_name.contains("age2") && !lc_name.contains("fakehost") {
            continue;
        }

        let outpath = outdir.join("dlls").join(name);
        fs::write(outpath, file)?;
    }

    Ok(())
}

fn launcher_companion_full_url(ctx: &Context) -> Result<Option<String>> {
    // https://api.github.com/repos/luskaner/ageLANServerLauncherCompanion/releases/latest
    // https://api.github.com/repos/luskaner/ageLANServerLauncherCompanion/releases/latest

    ctx.working_on("Getting latest launcher companion release url.");

    let url = format!(
        "https://api.github.com/repos/{}/{}/releases/latest",
        ctx.config.aoe2.gh_user, ctx.config.aoe2.gh_repo
    );

    // Ask the api for the latest release download
    let client = reqwest::blocking::Client::new();
    let json = client
        .get(url)
        .header(
            "User-Agent",
            "Mozilla/5.0 (Windows NT 10.0; Win64; x64; rv:143.0) Gecko/20100101 Firefox/143.0",
        )
        .send()?
        .text()?;
    let json: Value = serde_json::from_str(&json)?;

    let Some(assets) = json.get("assets") else {
        bail!("Unexpected response from github: expected assets field.");
    };
    let Some(assets) = assets.as_array() else {
        bail!("Expected github assets to be an array, but it was not.");
    };

    for asset in assets {
        let Some(name) = asset.get("name").and_then(|n| n.as_str()) else {
            continue;
        };
        if !name.contains("_full_") {
            continue;
        }

        let Some(url) = asset.get("browser_download_url").and_then(|u| u.as_str()) else {
            continue;
        };

        return Ok(Some(url.to_string()));
    }

    Ok(None)
}
