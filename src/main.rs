#![windows_subsystem = "windows"]

mod aoe;
mod config;
mod ctx;
mod goldberg;
mod steam;
mod ui;
mod utils;

use crate::aoe::aoe2;
use crate::ctx::{Context, StepStatus, Task};
use crate::steam::steam_aoe2_path;
use crate::ui::UiLayer;
use crate::utils::{desktop_dir, validate_aoe2_source};
use anyhow::{Context as AnyhowContext, Result};
use config::Config;
use eframe::egui;
use fs_extra::copy_items;
use fs_extra::dir::{CopyOptions, get_size};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::{Receiver, channel};
use std::sync::{Arc, Mutex};
use std::thread::sleep;
use std::time::Duration;
use tracing::{error, info};
use tracing_subscriber::layer::SubscriberExt;

struct App {
    pub update_rx: Receiver<AppUpdate>,
    pub state: Option<String>,
    pub error: Option<String>,
    pub progress: Option<(String, f32)>,
    pub logs: Vec<String>,
    pub disk_space_info: Option<(u64, u64)>, // (required, available)
    pub ctx: Arc<Context>,
}

impl App {
    fn add_log(&mut self, msg: String) {
        self.logs.push(msg);
        if self.logs.len() > 100 {
            self.logs.remove(0);
        }
    }
}

#[derive(Default)]
enum AppUpdate {
    #[default]
    Idle,
    Progress(Option<(String, f32)>),
    StepStatusChanged,
    DiskSpaceInfo(u64, u64),
    Log(String),
}

fn main() -> Result<()> {
    let config = Config::load()?;
    let (update_tx, update_rx) = channel();

    // Set up tracing to pipe logs to the UI
    let ui_layer = UiLayer {
        tx: update_tx.clone(),
    };

    let subscriber = tracing_subscriber::fmt()
        .with_max_level(tracing::Level::INFO)
        .with_target(false)
        .finish()
        .with(ui_layer);

    tracing::subscriber::set_global_default(subscriber).expect("Failed to set tracing subscriber");

    // Load icon from assets
    let icon_data = include_bytes!("../assets/aoe2.ico");
    let icon = match image::load_from_memory(icon_data) {
        Ok(img) => {
            let rgba = img.to_rgba8();
            let (width, height) = rgba.dimensions();
            Some(egui::IconData {
                rgba: rgba.into_raw(),
                width,
                height,
            })
        }
        Err(e) => {
            eprintln!("Failed to load icon: {}", e);
            None
        }
    };

    let mut viewport = egui::ViewportBuilder::default()
        .with_inner_size([700.0, 600.0])
        .with_min_inner_size([600.0, 500.0])
        .with_resizable(true);

    if let Some(icon) = icon {
        viewport = viewport.with_icon(icon);
    }

    let options = eframe::NativeOptions {
        viewport,
        ..Default::default()
    };

    let ctx = Context {
        config,
        tx: update_tx,
        outdir: Mutex::new(desktop_dir()?.join("AoE2")),
        source_dir: Mutex::new(steam_aoe2_path()?),
        current_task: Mutex::default(),
        step_status: Mutex::new([const { StepStatus::NotStarted }; 4]),
    };

    let app = App {
        state: None,
        error: None,
        update_rx,
        progress: None,
        logs: Vec::new(),
        disk_space_info: None,
        ctx: Arc::new(ctx),
    };

    if let Err(err) = eframe::run_native(
        "AoE2 DE Archiver",
        options,
        Box::new(|_cc| Ok(Box::new(app))),
    ) {
        println!("{err:?}");
    };

    Ok(())
}

fn spawn_copy_game_folder(app: &mut App) -> Result<()> {
    let guard = app.ctx.set_task(Task::Copy)?;
    let ctx = app.ctx.clone();

    // Validate source directory
    let source = app.ctx.source_dir()?;
    if source.is_none() {
        error!("No source directory selected");
        return Ok(());
    }

    std::thread::spawn({
        move || {
            let _guard = guard;
            ctx.set_step_status(0, StepStatus::InProgress);

            match copy_game_folder(ctx.clone()) {
                Ok(_) => {
                    ctx.set_step_status(0, StepStatus::Completed);
                    info!("Copy completed successfully");
                }
                Err(err) => {
                    let err_msg = format!("{:#}", err);
                    ctx.set_step_status(0, StepStatus::Failed(err_msg.clone()));
                    error!("Copy failed: {err_msg}");
                }
            }
        }
    });

    Ok(())
}

fn copy_game_folder(ctx: Arc<Context>) -> Result<()> {
    info!("Preparing to copy AoE2 files");

    let outdir = ctx.outdir()?;
    let source_aoe2_dir = ctx
        .source_dir()?
        .ok_or_else(|| anyhow::anyhow!("No source directory"))?;

    // Validate source
    validate_aoe2_source(&source_aoe2_dir).context("Source validation failed")?;

    // Get sizes and check disk space
    let dir_size = get_size(&source_aoe2_dir).context("Failed to get source directory size")?;

    // Note: A proper implementation would check actual available disk space using Windows API

    ctx.tx
        .send(AppUpdate::DiskSpaceInfo(dir_size, dir_size * 2))
        .ok();

    info!(
        "Copying from {} ({:.2} GB)",
        source_aoe2_dir.display(),
        dir_size as f64 / 1_073_741_824.0
    );

    std::fs::create_dir_all(&outdir).context("Failed to create destination directory")?;

    let complete = Arc::new(AtomicBool::new(false));

    // Progress monitoring thread
    std::thread::spawn({
        let ctx = ctx.clone();
        let outdir = outdir.clone();
        let complete = complete.clone();
        move || {
            loop {
                if complete.load(Ordering::Relaxed) {
                    break;
                }

                if let Ok(dest_size) = get_size(&outdir) {
                    let pct_complete = (dest_size as f64 / dir_size as f64).min(1.0) as f32;
                    let _ = ctx.tx.send(AppUpdate::Progress(Some((
                        format!("Copying... {:.1}%", pct_complete * 100.0),
                        pct_complete,
                    ))));
                }

                sleep(Duration::from_millis(500));
            }
        }
    });

    // Perform the copy
    let copy_options = CopyOptions::new();
    let from_paths = vec![source_aoe2_dir];
    copy_items(&from_paths, &outdir, &copy_options).context("Failed to copy files")?;

    complete.store(true, Ordering::Relaxed);
    ctx.tx.send(AppUpdate::Progress(None)).ok();

    info!("Copy completed successfully");

    Ok(())
}

fn spawn_run_all_steps(app: &mut App) -> Result<()> {
    let ctx = app.ctx.clone();
    std::thread::spawn({
        move || {
            // Step 1: Copy
            ctx.set_step_status(0, StepStatus::InProgress);

            match copy_game_folder(ctx.clone()) {
                Ok(_) => {
                    ctx.set_step_status(0, StepStatus::Completed);
                    info!("Step 1/4 completed: Game files copied");
                }
                Err(err) => {
                    let err_msg = format!("{:#}", err);
                    ctx.set_step_status(0, StepStatus::Failed(err_msg.clone()));
                    error!("Step 1 failed: {err_msg}");
                    return;
                }
            }

            // Step 2: Goldberg
            ctx.set_step_status(1, StepStatus::InProgress);
            match goldberg::apply_goldberg(ctx.clone()) {
                Ok(_) => {
                    ctx.set_step_status(1, StepStatus::Completed);
                    info!("Step 2/4 completed: Goldberg emulator applied");
                }
                Err(err) => {
                    let err_msg = format!("{:#}", err);
                    ctx.set_step_status(1, StepStatus::Failed(err_msg.clone()));
                    error!("Step 2 failed: {err_msg:#}");
                    return;
                }
            }

            // Step 3: Companion
            ctx.set_step_status(2, StepStatus::InProgress);
            match aoe2::companion::install_launcher_companion(ctx.clone()) {
                Ok(_) => {
                    ctx.set_step_status(2, StepStatus::Completed);
                    info!("Step 3/4 completed: Companion installed");
                }
                Err(err) => {
                    let err_msg = format!("{:#}", err);
                    ctx.set_step_status(2, StepStatus::Failed(err_msg.clone()));
                    error!("Step 3 failed: {err_msg}");
                    return;
                }
            }

            sleep(Duration::from_millis(500));

            // Step 4: Launcher
            ctx.set_step_status(3, StepStatus::InProgress);
            match aoe2::launcher::install_launcher(ctx.clone()) {
                Ok(_) => {
                    ctx.set_step_status(3, StepStatus::Completed);
                    info!("All steps completed successfully! ✓");
                }
                Err(err) => {
                    let err_msg = format!("{:#}", err);
                    ctx.set_step_status(3, StepStatus::Failed(err_msg.clone()));
                    error!("Step 4 failed: {err_msg}");
                }
            }
        }
    });

    Ok(())
}
