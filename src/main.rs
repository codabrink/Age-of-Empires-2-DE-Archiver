mod aoe;
mod config;
mod goldberg;
mod steam;
mod utils;

use crate::aoe::aoe2;
use crate::steam::steam_aoe2_path;
use crate::utils::desktop_dir;
use anyhow::{Result, bail};
use config::Config;
use eframe::egui::{self, Button, ProgressBar, TextEdit, Ui, ViewportCommand};
use fs_extra::copy_items;
use fs_extra::dir::{CopyOptions, get_size};
use std::path::{Path, PathBuf};
use std::pin::Pin;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::{Receiver, Sender, channel};
use std::thread::sleep;
use std::time::Duration;
use tracing::info;

struct App {
    initial_frame: bool,
    pub config: Arc<Config>,
    pub update_tx: Sender<AppUpdate>,
    pub update_rx: Receiver<AppUpdate>,
    pub state: Option<String>,
    pub progress: Option<f32>,
    pub source_dir: Pin<Box<Option<PathBuf>>>,
    pub outdir: Pin<Box<PathBuf>>,
}

struct Context {
    pub config: Arc<Config>,
    pub tx: Sender<AppUpdate>,
    pub source_dir: Option<*const PathBuf>,
    pub outdir: *const PathBuf,
}
unsafe impl Send for Context {}
unsafe impl Sync for Context {}

impl App {
    pub fn context(&self) -> Arc<Context> {
        Arc::new(Context {
            config: self.config.clone(),
            tx: self.update_tx.clone(),
            outdir: &*self.outdir,
            source_dir: (*self.source_dir).as_ref().map(|sd| &*sd as *const PathBuf),
        })
    }
}

impl Context {
    fn outdir(&self) -> Result<&Path> {
        Ok(unsafe { &*self.outdir })
    }

    pub fn working_on(&self, msg: impl ToString) {
        let msg = msg.to_string();
        println!("{msg}");
        let _ = self.tx.send(AppUpdate::Working(msg));
    }
}

#[derive(Default)]
enum AppUpdate {
    #[default]
    Idle,
    Working(String),
    Progress(Option<f32>),
}

impl eframe::App for App {
    fn update(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        while let Ok(state) = self.update_rx.try_recv() {
            match state {
                AppUpdate::Working(state) => self.state = Some(state),
                AppUpdate::Progress(pct) => self.progress = pct,
                _ => {}
            }
        }

        egui::CentralPanel::default().show(ctx, |ui| {
            let content_size = egui::Area::new("content_area".into())
                .fixed_pos(egui::pos2(8., 8.))
                .show(ui.ctx(), |ui| {
                    draw_main(self, ui);
                    ui.min_rect().size()
                })
                .inner;

            if self.initial_frame {
                let padded_size = content_size + egui::vec2(16., 16.);
                ctx.send_viewport_cmd(ViewportCommand::InnerSize(padded_size));
                self.initial_frame = false;
            }
        });
    }
}

fn folder_selection<F>(ui: &mut Ui, label: &str, val: Option<*const PathBuf>, mut callback: F)
where
    F: FnMut(PathBuf) -> (),
{
    unsafe {
        let mut text_val = val
            .map(|v| (*v).to_str().unwrap_or_default().to_string())
            .unwrap();
        ui.group(|ui| {
            ui.label(label);
            ui.add(TextEdit::singleline(&mut text_val).interactive(false));
            if ui.button("📁 Select Folder").clicked() {
                let mut dialog = rfd::FileDialog::new();
                if let Some(val) = val {
                    dialog = dialog.set_directory((*val).clone());
                };
                if let Some(dir) = dialog.pick_folder() {
                    callback(dir);
                }
            }
        });
    }
}

fn draw_main(app: &mut App, ui: &mut Ui) {
    ui.heading("AoE2");

    if let Some(progress) = app.progress {
        let progress_bar = ProgressBar::new(progress);
        ui.add(progress_bar);
    }

    let source_dir = (&*app.source_dir).as_ref().map(|sd| sd as *const PathBuf);
    folder_selection(ui, "AoE2 DE Source Dir", source_dir, |dir| {
        info!("Selected AoE2 DE Source directory: {}", dir.display());
        *app.source_dir = Some(dir);
    });

    folder_selection(ui, "Destination Dir", Some(&*app.outdir), |dir| {
        info!("Selected destination directory: {}", dir.display());
        *app.outdir = dir
    });

    let btn_export = Button::new("Create Package");
    if ui
        .add_enabled(app.source_dir.is_some(), btn_export)
        .clicked()
    {
        start_export(app);
    }
    if ui.button("Apply Goldberg Emulator").clicked() {
        goldberg::apply(app.context());
    }

    if ui.button("Install companion").clicked() {
        if let Err(err) = aoe2::companion::install_launcher_companion(app.context()) {
            dbg!(err);
        };
    }

    if ui.button("Install launcher").clicked() {
        if let Err(err) = aoe2::launcher::install_launcher(app.context()) {
            dbg!(err);
        }
    }

    if let Some(state) = &app.state {
        ui.label(state);
        ui.end_row();
    }
}

fn main() -> Result<()> {
    tracing_subscriber::fmt::init();

    let options = eframe::NativeOptions {
        viewport: egui::ViewportBuilder {
            // resizable: Some(false),
            ..Default::default()
        },
        ..Default::default()
    };

    let config = Config::load()?;
    let (update_tx, update_rx) = channel();
    let app = App {
        initial_frame: true,
        config: Arc::new(config),
        state: None,
        update_tx,
        update_rx,
        progress: None,
        outdir: Box::pin(desktop_dir()?.join("AoE2")),
        source_dir: Box::pin(steam_aoe2_path()?),
    };

    if let Err(err) = eframe::run_native("Aoe2 DE", options, Box::new(|_cc| Ok(Box::new(app)))) {
        println!("{err:?}");
    };

    Ok(())
}

fn start_export(app: &mut App) {
    println!("Starting export.");

    std::thread::spawn({
        let ctx = app.context();
        move || {
            if let Err(err) = export(ctx) {
                // handle
            }
        }
    });
}

fn export(ctx: Arc<Context>) -> Result<()> {
    ctx.working_on("Copying AoE2 to new folder");

    let outdir = ctx.outdir()?;
    let Some(source_aoe2_dir) = (unsafe { ctx.source_dir.map(|d| &*d) }) else {
        bail!("Missing source dir");
    };

    let _ = std::fs::remove_dir_all(&outdir);
    let _ = std::fs::create_dir_all(&outdir);

    let dir_size = get_size(&source_aoe2_dir)?;
    ctx.working_on(format!(
        "Copying from {source_aoe2_dir:?} ({dir_size} bytes)"
    ));

    let complete = Arc::new(AtomicBool::new(false));
    std::thread::spawn({
        let ctx = ctx.clone();
        let outdir = outdir.to_path_buf();
        let complete = complete.clone();
        move || {
            loop {
                if complete.load(Ordering::Relaxed) {
                    break;
                }

                let dest_size = get_size(&outdir).unwrap();
                let pct_complete = dest_size as f32 / dir_size as f32;

                dbg!(pct_complete);

                let _ = ctx.tx.send(AppUpdate::Progress(Some(pct_complete)));

                sleep(Duration::from_secs(1));
            }
        }
    });

    let copy_options = CopyOptions::new();
    let from_paths = vec![source_aoe2_dir];
    copy_items(&from_paths, &outdir, &copy_options)?;

    complete.store(true, Ordering::Relaxed);
    ctx.tx.send(AppUpdate::Progress(None));

    ctx.working_on("Done copying.");

    Ok(())
}
