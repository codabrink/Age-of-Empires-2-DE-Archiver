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
use crate::ui::UiLayer;
use crate::utils::validate_aoe2_source;
use anyhow::{Context as AnyhowContext, Result, bail};
use eframe::egui;
use fs_extra::copy_items;
use fs_extra::dir::{CopyOptions, get_size};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::{Receiver, RecvError, channel};
use std::sync::{Arc, mpsc};
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
    pub required_space: Option<u64>,
    pub available_space: Option<u64>,
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
    SourceSize(u64),
    DestDriveAvailable(u64),
    Log(String),
}

fn main() -> Result<()> {
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

    let app = App {
        state: None,
        error: None,
        update_rx,
        progress: None,
        logs: Vec::new(),
        required_space: None,
        available_space: None,
        ctx: Arc::new(Context::new(update_tx)?),
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

fn spawn_copy_game_folder(ctx: Arc<Context>) -> Result<Receiver<()>> {
    let guard = ctx.set_task(Task::Copy)?;
    let ctx = ctx.clone();

    let (tx, rx) = mpsc::sync_channel(0);

    // Validate source directory
    let source = ctx.sourcedir();
    if source.is_none() {
        bail!("No source directory selected");
    }

    std::thread::spawn({
        move || {
            let _guard = guard;
            ctx.set_step_status(0, StepStatus::InProgress);

            match copy_game_folder(ctx.clone()) {
                Ok(_) => {
                    ctx.set_step_status(0, StepStatus::Completed);
                    info!("Copy completed successfully");
                    let _ = tx.send(());
                }
                Err(err) => {
                    let err_msg = format!("{:#}", err);
                    ctx.set_step_status(0, StepStatus::Failed(err_msg.clone()));
                    error!("Copy failed: {err_msg}");
                }
            }
        }
    });

    Ok(rx)
}

fn copy_game_folder(ctx: Arc<Context>) -> Result<()> {
    info!("Preparing to copy AoE2 files");

    let outdir = ctx.outdir();
    let source_aoe2_dir = ctx
        .sourcedir()
        .ok_or_else(|| anyhow::anyhow!("No source directory"))?;

    // Validate source
    validate_aoe2_source(&source_aoe2_dir).context("Source validation failed")?;

    // Get sizes and check disk space
    let dir_size = get_size(&source_aoe2_dir).context("Failed to get source directory size")?;

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

fn run_all_steps(ctx: Arc<Context>) {
    std::thread::spawn({
        move || {
            if let Err(err) = run_all_steps_inner(ctx) {
                // Don't log recv errors.
                let Err(err) = err.downcast::<RecvError>() else {
                    return;
                };
                error!("{err:?}");
            }
        }
    });
}

fn run_all_steps_inner(ctx: Arc<Context>) -> Result<()> {
    // Step 1: Copy
    ctx.set_step_status(0, StepStatus::InProgress);
    let rx = spawn_copy_game_folder(ctx.clone())?;
    rx.recv()?;
    info!("Step 1/4 completed: Game files copied");

    // Step 2: Goldberg
    ctx.set_step_status(1, StepStatus::InProgress);
    let rx = goldberg::spawn_apply(ctx.clone())?;
    rx.recv()?;
    info!("Step 2/4 completed: Goldberg installed");

    // Step 3: Companion
    ctx.set_step_status(2, StepStatus::InProgress);
    let rx = aoe2::companion::spawn_install_launcher_companion(ctx.clone())?;
    rx.recv()?;
    info!("Step 3/4 completed: Launcher Companion Installed");

    // Step 4: Launcher
    ctx.set_step_status(3, StepStatus::InProgress);
    let rx = aoe2::launcher::spawn_install_launcher(ctx.clone())?;

    rx.recv()?;
    info!("Step 4/4 completed: Launcher Installed");

    Ok(())
}
