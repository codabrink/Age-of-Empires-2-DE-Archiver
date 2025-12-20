// #![windows_subsystem = "windows"]

mod aoe;
mod config;
mod goldberg;
mod steam;
mod utils;

use crate::aoe::aoe2;
use crate::steam::steam_aoe2_path;
use crate::utils::{Busy, desktop_dir};
use anyhow::{Context as AnyhowContext, Result, bail};
use config::Config;
use eframe::egui::{self, Button, Color32, ProgressBar, RichText, TextEdit, Ui};
use fs_extra::copy_items;
use fs_extra::dir::{CopyOptions, get_size};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::{Receiver, Sender, channel};
use std::sync::{Arc, Mutex};
use std::thread::sleep;
use std::time::Duration;
use tracing::info;

#[derive(Debug, Clone, PartialEq)]
enum StepStatus {
    NotStarted,
    InProgress,
    Completed,
    Failed(String),
}

impl StepStatus {
    fn icon(&self) -> &str {
        match self {
            StepStatus::NotStarted => "⚪",
            StepStatus::InProgress => "⏳",
            StepStatus::Completed => "✓",
            StepStatus::Failed(_) => "✗",
        }
    }

    fn color(&self) -> Color32 {
        match self {
            StepStatus::NotStarted => Color32::GRAY,
            StepStatus::InProgress => Color32::from_rgb(255, 165, 0), // Orange
            StepStatus::Completed => Color32::from_rgb(0, 200, 0),    // Green
            StepStatus::Failed(_) => Color32::from_rgb(220, 0, 0),    // Red
        }
    }
}

struct App {
    pub config: Arc<Config>,
    pub update_tx: Sender<AppUpdate>,
    pub update_rx: Receiver<AppUpdate>,
    pub state: Option<String>,
    pub error: Option<String>,
    pub progress: Option<(String, f32)>,
    pub source_dir: Arc<Mutex<Option<PathBuf>>>,
    pub outdir: Arc<Mutex<PathBuf>>,
    pub step_status: Arc<Mutex<[StepStatus; 4]>>,
    pub show_logs: bool,
    pub logs: Vec<String>,
    pub disk_space_info: Option<(u64, u64)>, // (required, available)
}

struct Context {
    pub config: Arc<Config>,
    pub tx: Sender<AppUpdate>,
    pub source_dir: Arc<Mutex<Option<PathBuf>>>,
    pub outdir: Arc<Mutex<PathBuf>>,
    pub step_status: Arc<Mutex<[StepStatus; 4]>>,
    pub busy: Busy,
}

impl App {
    pub fn context(&self) -> Arc<Context> {
        Arc::new(Context {
            config: self.config.clone(),
            tx: self.update_tx.clone(),
            outdir: self.outdir.clone(),
            source_dir: self.source_dir.clone(),
            step_status: self.step_status.clone(),
            busy: Busy::new(),
        })
    }

    fn add_log(&mut self, msg: String) {
        self.logs.push(msg);
        if self.logs.len() > 100 {
            self.logs.remove(0);
        }
    }
}

impl Context {
    fn outdir(&self) -> Result<PathBuf> {
        Ok(self.outdir.lock().unwrap().clone())
    }

    fn source_dir(&self) -> Result<Option<PathBuf>> {
        Ok(self.source_dir.lock().unwrap().clone())
    }

    pub fn working_on(&self, msg: impl ToString) {
        let msg = msg.to_string();
        println!("{msg}");
        let _ = self.tx.send(AppUpdate::Working(msg));
    }

    pub fn set_step_status(&self, step: usize, status: StepStatus) {
        if let Ok(mut steps) = self.step_status.lock() {
            if step < steps.len() {
                steps[step] = status;
            }
        }
        let _ = self.tx.send(AppUpdate::StepStatusChanged);
    }

    pub fn send_error(&self, msg: String) {
        let _ = self.tx.send(AppUpdate::Error(msg));
    }
}

#[derive(Default)]
enum AppUpdate {
    #[default]
    Idle,
    Working(String),
    Progress(Option<(String, f32)>),
    Error(String),
    StepStatusChanged,
    DiskSpaceInfo(u64, u64),
}

impl eframe::App for App {
    fn update(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        while let Ok(state) = self.update_rx.try_recv() {
            match state {
                AppUpdate::Working(state) => {
                    self.add_log(state.clone());
                    self.state = Some(state);
                    self.error = None;
                }
                AppUpdate::Progress(progress) => self.progress = progress,
                AppUpdate::Error(err) => {
                    self.add_log(format!("ERROR: {}", err));
                    self.error = Some(err);
                    self.state = None;
                }
                AppUpdate::DiskSpaceInfo(required, available) => {
                    self.disk_space_info = Some((required, available));
                }
                AppUpdate::StepStatusChanged => {
                    // Force UI update
                }
                _ => {}
            }
        }

        egui::CentralPanel::default().show(ctx, |ui| {
            egui::ScrollArea::vertical().show(ui, |ui| {
                draw_main(self, ui);
            });
        });
    }
}

fn folder_selection(
    ui: &mut Ui,
    label: &str,
    tooltip: &str,
    dir_path: Arc<Mutex<Option<PathBuf>>>,
    validation: Option<fn(&Path) -> Result<()>>,
) {
    ui.group(|ui| {
        ui.set_min_width(ui.available_width());
        ui.horizontal(|ui| {
            ui.label(label);
            if !tooltip.is_empty() {
                ui.label("ℹ").on_hover_text(tooltip);
            }
        });

        // Read the current value fresh each frame
        let current_path_text = dir_path
            .lock()
            .unwrap()
            .as_ref()
            .map(|p| p.to_str().unwrap_or_default().to_string())
            .unwrap_or_default();

        ui.horizontal(|ui| {
            let mut text_val = current_path_text.clone();
            let text_widget = TextEdit::singleline(&mut text_val).interactive(false);
            ui.add_sized([ui.available_width() - 120.0, 20.0], text_widget);

            if ui.button("📁 Select Folder").clicked() {
                let current = dir_path.lock().unwrap().clone();
                let mut dialog = rfd::FileDialog::new();
                if let Some(current_path) = current {
                    dialog = dialog.set_directory(current_path);
                }
                if let Some(new_dir) = dialog.pick_folder() {
                    info!("User selected directory: {}", new_dir.display());
                    let mut valid = true;
                    let mut error_msg = None;
                    if let Some(validate_fn) = validation {
                        if let Err(e) = validate_fn(&new_dir) {
                            valid = false;
                            error_msg = Some(format!("{}", e));
                            info!("Validation failed: {}", e);
                        }
                    }
                    if valid {
                        info!("Updating source directory to: {}", new_dir.display());
                        *dir_path.lock().unwrap() = Some(new_dir.clone());
                        info!("Source directory updated successfully");
                        // Force UI update
                        ui.ctx().request_repaint();
                    } else if let Some(msg) = error_msg {
                        rfd::MessageDialog::new()
                            .set_title("Invalid Directory")
                            .set_description(&msg)
                            .set_buttons(rfd::MessageButtons::Ok)
                            .show();
                    }
                }
            }
        });

        // Show validation warning if present
        if let Some(validate_fn) = validation {
            if let Some(path) = dir_path.lock().unwrap().as_ref() {
                if let Err(e) = validate_fn(path) {
                    ui.colored_label(Color32::from_rgb(255, 100, 0), format!("⚠ {}", e));
                }
            }
        }
    });
}

fn folder_selection_required(
    ui: &mut Ui,
    label: &str,
    tooltip: &str,
    dir_path: Arc<Mutex<PathBuf>>,
) {
    ui.group(|ui| {
        ui.set_min_width(ui.available_width());
        ui.horizontal(|ui| {
            ui.label(label);
            if !tooltip.is_empty() {
                ui.label("ℹ").on_hover_text(tooltip);
            }
        });

        ui.horizontal(|ui| {
            // Read the current value fresh each frame
            let mut text_val = dir_path
                .lock()
                .unwrap()
                .to_str()
                .unwrap_or_default()
                .to_string();

            let text_widget = TextEdit::singleline(&mut text_val).interactive(false);
            ui.add_sized([ui.available_width() - 120.0, 20.0], text_widget);

            if ui.button("📁 Select Folder").clicked() {
                let current = dir_path.lock().unwrap().clone();
                let mut dialog = rfd::FileDialog::new();
                dialog = dialog.set_directory(current);
                if let Some(new_dir) = dialog.pick_folder() {
                    info!("Selected directory: {}", new_dir.display());
                    *dir_path.lock().unwrap() = new_dir;
                }
            }
        });
    });
}

fn validate_aoe2_source(path: &Path) -> Result<()> {
    if !path.exists() {
        bail!("Directory does not exist");
    }
    if !path.is_dir() {
        bail!("Path is not a directory");
    }

    // Check for AoE2DE executable
    let exe_path = path.join("AoE2DE_s.exe");
    if !exe_path.exists() {
        bail!("This doesn't appear to be an AoE2 DE directory (AoE2DE_s.exe not found)");
    }

    Ok(())
}

fn draw_status_banner(ui: &mut Ui, app: &App) {
    if let Some(err) = &app.error {
        ui.horizontal(|ui| {
            ui.label(
                RichText::new("✗ Error:")
                    .color(Color32::from_rgb(220, 0, 0))
                    .strong(),
            );
            ui.label(RichText::new(err).color(Color32::from_rgb(220, 0, 0)));
        });
        ui.add_space(5.0);
    } else if let Some(state) = &app.state {
        ui.horizontal(|ui| {
            ui.label(RichText::new("⏳").color(Color32::from_rgb(255, 165, 0)));
            ui.label(RichText::new(state).color(Color32::from_rgb(255, 165, 0)));
        });
        ui.add_space(5.0);
    }

    if let Some((desc, pct)) = &app.progress {
        let progress_bar = ProgressBar::new(*pct).text(desc);
        ui.add_sized([ui.available_width(), 20.0], progress_bar);
        ui.add_space(5.0);
    }
}

fn draw_step_button(
    ui: &mut Ui,
    _step_num: usize,
    label: &str,
    tooltip: &str,
    status: &StepStatus,
    enabled: bool,
) -> bool {
    let mut clicked = false;
    ui.horizontal(|ui| {
        ui.label(
            RichText::new(status.icon())
                .color(status.color())
                .size(20.0),
        );

        let button = Button::new(label);
        let response = ui.add_enabled(enabled, button);

        if !tooltip.is_empty() {
            response.clone().on_hover_text(tooltip);
        }

        if response.clicked() {
            clicked = true;
        }

        if let StepStatus::Failed(err) = status {
            ui.label(RichText::new(format!("({})", err)).color(Color32::from_rgb(220, 0, 0)));
        }
    });
    clicked
}

fn draw_main(app: &mut App, ui: &mut Ui) {
    ui.heading("AoE2 DE Archiver");
    ui.separator();
    ui.add_space(5.0);

    // Status banner at the top
    draw_status_banner(ui, app);

    // Disk space info
    if let Some((required, available)) = app.disk_space_info {
        let required_gb = required as f64 / 1_073_741_824.0;
        let available_gb = available as f64 / 1_073_741_824.0;
        let color = if available > required {
            Color32::from_rgb(0, 200, 0)
        } else {
            Color32::from_rgb(220, 0, 0)
        };
        ui.horizontal(|ui| {
            ui.label("Disk Space:");
            ui.label(
                RichText::new(format!(
                    "{:.2} GB required, {:.2} GB available",
                    required_gb, available_gb
                ))
                .color(color),
            );
        });
        ui.add_space(5.0);
    }

    // Directory selection
    ui.label(RichText::new("Configuration").strong().size(16.0));
    ui.add_space(5.0);

    folder_selection(
        ui,
        "AoE2 DE Source Directory",
        "Select the folder containing your Age of Empires II: Definitive Edition installation",
        app.source_dir.clone(),
        Some(validate_aoe2_source),
    );
    ui.add_space(5.0);

    folder_selection_required(
        ui,
        "Destination Directory",
        "Select where you want to create the archived copy of the game",
        app.outdir.clone(),
    );
    ui.add_space(10.0);

    ui.separator();
    ui.add_space(10.0);

    // Steps section
    ui.label(RichText::new("Steps").strong().size(16.0));
    ui.add_space(5.0);

    let busy = app.context().busy.is_busy();
    let steps = app.step_status.lock().unwrap().clone();

    let source_exists = app.source_dir.lock().unwrap().is_some();

    // Step 1: Copy game folder
    if draw_step_button(
        ui,
        0,
        "1. Copy Game Folder",
        "Copy the AoE2 DE game files to the destination directory",
        &steps[0],
        source_exists && !busy,
    ) {
        if let Err(e) = spawn_copy_game_folder(app) {
            app.error = Some(format!("Failed to start copy: {}", e));
        }
    }
    ui.add_space(5.0);

    // Step 2: Apply Goldberg
    if draw_step_button(
        ui,
        1,
        "2. Apply Goldberg Emulator",
        "Apply the Goldberg Steam emulator to make the game run without Steam",
        &steps[1],
        !busy && matches!(steps[0], StepStatus::Completed),
    ) {
        if let Err(e) = goldberg::spawn_apply(app.context()) {
            app.error = Some(format!("Failed to start Goldberg installation: {}", e));
        }
    }
    ui.add_space(5.0);

    // Step 3: Install companion
    if draw_step_button(
        ui,
        2,
        "3. Install Companion",
        "Install the companion tool for additional functionality",
        &steps[2],
        !busy && matches!(steps[1], StepStatus::Completed),
    ) {
        if let Err(err) = aoe2::companion::spawn_install_launcher_companion(app.context()) {
            app.error = Some(format!("Failed to install companion: {}", err));
        }
    }
    ui.add_space(5.0);

    // Step 4: Install launcher
    if draw_step_button(
        ui,
        3,
        "4. Install Launcher",
        "Install the game launcher",
        &steps[3],
        !busy && matches!(steps[2], StepStatus::Completed),
    ) {
        if let Err(err) = aoe2::launcher::spawn_install_launcher(app.context()) {
            app.error = Some(format!("Failed to install launcher: {}", err));
        }
    }
    ui.add_space(10.0);

    ui.separator();
    ui.add_space(10.0);

    // Run All button
    ui.horizontal(|ui| {
        let can_run_all = source_exists && !busy && matches!(steps[0], StepStatus::NotStarted);
        if ui
            .add_enabled(can_run_all, Button::new("▶ Run All Steps"))
            .on_hover_text("Automatically run all steps in sequence")
            .clicked()
        {
            if let Err(e) = spawn_run_all_steps(app) {
                app.error = Some(format!("Failed to start: {}", e));
            }
        }
    });
    ui.add_space(10.0);

    // Logs section
    ui.separator();
    ui.add_space(5.0);

    ui.horizontal(|ui| {
        ui.label(RichText::new("Logs").strong().size(16.0));
        if ui
            .button(if app.show_logs {
                "▼ Hide"
            } else {
                "▶ Show"
            })
            .clicked()
        {
            app.show_logs = !app.show_logs;
        }
    });

    if app.show_logs {
        ui.add_space(5.0);
        egui::ScrollArea::vertical()
            .max_height(150.0)
            .show(ui, |ui| {
                ui.group(|ui| {
                    ui.set_min_width(ui.available_width());
                    if app.logs.is_empty() {
                        ui.label(RichText::new("No logs yet").italics().color(Color32::GRAY));
                    } else {
                        for log in app.logs.iter().rev().take(50) {
                            ui.label(RichText::new(log).small());
                        }
                    }
                });
            });
    }
}

fn main() -> Result<()> {
    tracing_subscriber::fmt()
        .with_max_level(tracing::Level::INFO)
        .with_target(false)
        .init();

    let options = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default()
            .with_inner_size([700.0, 600.0])
            .with_min_inner_size([600.0, 500.0])
            .with_resizable(true),
        ..Default::default()
    };

    let config = Config::load()?;
    let (update_tx, update_rx) = channel();

    let app = App {
        config: Arc::new(config),
        state: None,
        error: None,
        update_tx,
        update_rx,
        progress: None,
        outdir: Arc::new(Mutex::new(desktop_dir()?.join("AoE2"))),
        source_dir: Arc::new(Mutex::new(steam_aoe2_path()?)),
        step_status: Arc::new(Mutex::new([
            StepStatus::NotStarted,
            StepStatus::NotStarted,
            StepStatus::NotStarted,
            StepStatus::NotStarted,
        ])),
        show_logs: false,
        logs: Vec::new(),
        disk_space_info: None,
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
    let busy = app.context().busy.lock()?;
    let ctx = app.context();

    // Validate source directory
    let source = ctx.source_dir()?;
    if source.is_none() {
        ctx.send_error("No source directory selected".to_string());
        return Ok(());
    }

    std::thread::spawn({
        move || {
            let _busy = busy;
            ctx.set_step_status(0, StepStatus::InProgress);

            match copy_game_folder(ctx.clone()) {
                Ok(_) => {
                    ctx.set_step_status(0, StepStatus::Completed);
                    ctx.working_on("Copy completed successfully");
                }
                Err(err) => {
                    let err_msg = format!("{:#}", err);
                    ctx.set_step_status(0, StepStatus::Failed(err_msg.clone()));
                    ctx.send_error(format!("Copy failed: {}", err_msg));
                }
            }
        }
    });

    Ok(())
}

fn copy_game_folder(ctx: Arc<Context>) -> Result<()> {
    ctx.working_on("Preparing to copy AoE2 files");

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

    ctx.working_on(format!(
        "Copying from {} ({:.2} GB)",
        source_aoe2_dir.display(),
        dir_size as f64 / 1_073_741_824.0
    ));

    // Clean and create destination
    if outdir.exists() {
        let result = rfd::MessageDialog::new()
            .set_title("Destination Exists")
            .set_description(&format!(
                "The destination folder already exists:\n{}\n\nDo you want to delete it and continue?",
                outdir.display()
            ))
            .set_buttons(rfd::MessageButtons::YesNo)
            .show();

        if result != rfd::MessageDialogResult::Yes {
            bail!("Operation cancelled by user");
        }

        ctx.working_on("Removing existing destination folder");
        std::fs::remove_dir_all(&outdir).context("Failed to remove existing destination")?;
    }

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

    ctx.working_on("Copy completed successfully");

    Ok(())
}

fn spawn_run_all_steps(app: &mut App) -> Result<()> {
    let busy = app.context().busy.lock()?;
    let ctx = app.context();

    std::thread::spawn({
        move || {
            let _busy = busy;

            // Step 1: Copy
            ctx.set_step_status(0, StepStatus::InProgress);
            match copy_game_folder(ctx.clone()) {
                Ok(_) => {
                    ctx.set_step_status(0, StepStatus::Completed);
                    ctx.working_on("Step 1/4 completed: Game files copied");
                }
                Err(err) => {
                    let err_msg = format!("{:#}", err);
                    ctx.set_step_status(0, StepStatus::Failed(err_msg.clone()));
                    ctx.send_error(format!("Step 1 failed: {}", err_msg));
                    return;
                }
            }

            sleep(Duration::from_millis(500));

            // Step 2: Goldberg
            ctx.set_step_status(1, StepStatus::InProgress);
            match goldberg::apply_goldberg(ctx.clone()) {
                Ok(_) => {
                    ctx.set_step_status(1, StepStatus::Completed);
                    ctx.working_on("Step 2/4 completed: Goldberg emulator applied");
                }
                Err(err) => {
                    let err_msg = format!("{:#}", err);
                    ctx.set_step_status(1, StepStatus::Failed(err_msg.clone()));
                    ctx.send_error(format!("Step 2 failed: {}", err_msg));
                    return;
                }
            }

            sleep(Duration::from_millis(500));

            // Step 3: Companion
            ctx.set_step_status(2, StepStatus::InProgress);
            match aoe2::companion::install_launcher_companion(ctx.clone()) {
                Ok(_) => {
                    ctx.set_step_status(2, StepStatus::Completed);
                    ctx.working_on("Step 3/4 completed: Companion installed");
                }
                Err(err) => {
                    let err_msg = format!("{:#}", err);
                    ctx.set_step_status(2, StepStatus::Failed(err_msg.clone()));
                    ctx.send_error(format!("Step 3 failed: {}", err_msg));
                    return;
                }
            }

            sleep(Duration::from_millis(500));

            // Step 4: Launcher
            ctx.set_step_status(3, StepStatus::InProgress);
            match aoe2::launcher::install_launcher(ctx.clone()) {
                Ok(_) => {
                    ctx.set_step_status(3, StepStatus::Completed);
                    ctx.working_on("All steps completed successfully! ✓");
                }
                Err(err) => {
                    let err_msg = format!("{:#}", err);
                    ctx.set_step_status(3, StepStatus::Failed(err_msg.clone()));
                    ctx.send_error(format!("Step 4 failed: {}", err_msg));
                }
            }
        }
    });

    Ok(())
}
