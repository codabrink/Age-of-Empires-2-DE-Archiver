use crate::{
    Context,
    utils::{extract_zip, gh_latest_release_dl_url},
};
use anyhow::{Result, bail};
use std::{fs, sync::Arc};

pub fn spawn_install_launcher_companion(ctx: Arc<Context>) -> Result<()> {
    let busy = ctx.busy.lock()?;

    std::thread::spawn(move || {
        let _busy = busy;
        ctx.set_step_status(2, crate::StepStatus::InProgress);
        match install_launcher_companion(ctx.clone()) {
            Ok(_) => {
                ctx.set_step_status(2, crate::StepStatus::Completed);
                ctx.working_on("Companion installed successfully");
            }
            Err(err) => {
                let err_msg = format!("{:#}", err);
                ctx.set_step_status(2, crate::StepStatus::Failed(err_msg.clone()));
                ctx.send_error(format!("Companion installation failed: {}", err_msg));
                tracing::error!("{err:?}");
            }
        }
    });

    Ok(())
}

pub fn install_launcher_companion(ctx: Arc<Context>) -> Result<()> {
    let Some(companion_full_url) = launcher_companion_full_url(&ctx)? else {
        bail!("Unable to find latest companion release");
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

    ctx.working_on("Done installing companion.");

    Ok(())
}

fn launcher_companion_full_url(ctx: &Context) -> Result<Option<String>> {
    ctx.working_on("Getting latest launcher companion release url.");
    gh_latest_release_dl_url(
        &ctx.config.aoe2.gh_companion_user,
        &ctx.config.aoe2.gh_companion_repo,
        &["_full_"],
    )
}
