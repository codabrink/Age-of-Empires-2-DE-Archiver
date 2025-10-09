mod aoe;
mod config;
mod goldberg;
mod steam;
mod utils;

use crate::aoe::aoe2;
use crate::steam::steam_aoe2_path;
use crate::utils::desktop_dir;
use anyhow::Result;
use config::Config;
use eframe::egui::{self, Ui};
use fs_extra::copy_items;
use fs_extra::dir::{CopyOptions, get_size};
use std::path::PathBuf;
use std::sync::Arc;
use std::sync::mpsc::{Receiver, Sender, channel};

struct App {
    pub config: Arc<Config>,
    pub update_tx: Sender<AppState>,
    pub update_rx: Receiver<AppState>,
    pub state: AppState,
}

struct Context {
    pub config: Arc<Config>,
    pub tx: Sender<AppState>,
}

impl App {
    pub fn context(&self) -> Context {
        Context {
            config: self.config.clone(),
            tx: self.update_tx.clone(),
        }
    }
}

impl Context {
    fn outdir(&self) -> Result<PathBuf> {
        Ok(desktop_dir()?.join("AoE2"))
    }

    pub fn working_on(&self, msg: impl ToString) {
        let msg = msg.to_string();
        println!("{msg}");
        let _ = self.tx.send(AppState::Working(msg));
    }
}

#[derive(Default)]
enum AppState {
    #[default]
    Idle,
    Working(String),
}

impl eframe::App for App {
    fn update(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        while let Ok(state) = self.update_rx.try_recv() {
            self.state = state;
        }

        egui::CentralPanel::default().show(ctx, |ui| {
            draw_main(self, ui);
        });
    }
}

fn draw_main(app: &mut App, ui: &mut Ui) {
    ui.heading("AoE2");
    ui.group(|ui| {
        if ui.button("Create Package").clicked() {
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

        if let AppState::Working(desc) = &app.state {
            ui.label(desc);
            ui.end_row();
        }
    });
}

fn main() -> Result<()> {
    let options = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default().with_inner_size([320., 240.]),
        ..Default::default()
    };

    let config = Config::load()?;
    let (update_tx, update_rx) = channel();
    let app = App {
        config: Arc::new(config),
        state: AppState::Idle,
        update_tx,
        update_rx,
    };

    if let Err(err) = eframe::run_native("Aoe2 DE", options, Box::new(|_cc| Ok(Box::new(app)))) {
        println!("{err:?}");
    };

    Ok(())
}

fn start_export(app: &mut App) {
    println!("Starting export.");
    let ctx = app.context();

    std::thread::spawn(move || {
        if let Err(err) = export(ctx) {
            // handle
        }
    });
}

fn export(ctx: Context) -> Result<()> {
    ctx.working_on("Copying AoE2 to new folder");

    let outdir = ctx.outdir()?;
    let source_aoe2_dir = steam_aoe2_path(&ctx)?;

    let _ = std::fs::remove_dir_all(&outdir);
    let _ = std::fs::create_dir_all(&outdir);

    let dir_size = get_size(&source_aoe2_dir)?;
    ctx.working_on(format!(
        "Copying from {source_aoe2_dir:?} ({dir_size} bytes)"
    ));

    let copy_options = CopyOptions::new();
    let from_paths = vec![source_aoe2_dir];
    copy_items(&from_paths, &outdir, &copy_options)?;

    ctx.working_on("Done copying.");

    Ok(())
}
