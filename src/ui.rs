use crate::{
    App, AppUpdate,
    ctx::{Context, StepStatus},
    run_all_steps,
    utils::validate_aoe2_source,
};
use anyhow::Result;
use eframe::egui::{self, Button, Color32, ProgressBar, RichText, TextEdit, Ui};
use std::{
    path::{Path, PathBuf},
    sync::mpsc::Sender,
};
use tracing::info;
use tracing_subscriber::Layer;

fn draw_main(app: &mut App, ui: &mut Ui) -> Result<()> {
    ui.heading("AoE2 DE Archiver");
    ui.separator();
    ui.add_space(10.0);

    // Status banner at the top
    draw_status_banner(ui, app);

    // Disk space info
    let required = app.required_space.unwrap_or_default() as f64;
    let available = app.available_space.unwrap_or_default() as f64;
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
    ui.add_space(10.0);
    ui.separator();

    ui.label(RichText::new("Configuration").strong().size(16.0));
    ui.add_space(8.0);

    folder_selection(
        ui,
        &app.ctx,
        "AoE2 DE Source Directory",
        "Select the folder containing your Age of Empires II: Definitive Edition installation",
        app.ctx.sourcedir(),
        Some(validate_aoe2_source),
    );
    ui.add_space(8.0);

    folder_selection_required(
        ui,
        &app.ctx,
        "Destination Directory",
        "Select where you want to create the archived copy of the game",
        app.ctx.outdir(),
    );
    ui.add_space(10.0);

    // Steps section
    ui.separator();
    ui.label(RichText::new("Steps").strong().size(16.0));
    ui.add_space(8.0);

    ui.horizontal(|ui| {
        let step_status = app.ctx.step_status.lock().unwrap();

        // Step 1: Copy
        ui.label(
            RichText::new(step_status[0].icon())
                .color(step_status[0].color())
                .size(18.0),
        );
        ui.label("1. Copy");
        ui.add_space(10.0);

        // Step 2: Goldberg
        ui.label(
            RichText::new(step_status[1].icon())
                .color(step_status[1].color())
                .size(18.0),
        );
        ui.label("2. Goldberg");
        ui.add_space(10.0);

        // Step 3: Companion
        ui.label(
            RichText::new(step_status[2].icon())
                .color(step_status[2].color())
                .size(18.0),
        );
        ui.label("3. Companion");
        ui.add_space(10.0);

        // Step 4: Launcher
        ui.label(
            RichText::new(step_status[3].icon())
                .color(step_status[3].color())
                .size(18.0),
        );
        ui.label("4. Launcher");
    });
    ui.add_space(10.0);

    // Run All button
    let source_exists = app.ctx.sourcedir().is_some();
    let can_run_all = source_exists
        && !app.ctx.is_busy()
        && app
            .ctx
            .step_status
            .lock()
            .unwrap()
            .iter()
            .all(|s| matches!(s, StepStatus::NotStarted));

    if ui
        .add_enabled(
            can_run_all,
            Button::new("▶ Run All Steps").min_size([150.0, 30.0].into()),
        )
        .on_hover_text("Automatically run all steps in sequence")
        .clicked()
    {
        run_all_steps(app.ctx.clone());
    }
    ui.add_space(10.0);

    // Logs section
    ui.separator();
    ui.label(RichText::new("Logs").strong().size(16.0));
    ui.add_space(8.0);

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

    Ok(())
}

impl eframe::App for App {
    fn update(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        while let Ok(state) = self.update_rx.try_recv() {
            match state {
                AppUpdate::Progress(progress) => self.progress = progress,
                AppUpdate::SourceSize(required) => {
                    self.required_space = Some(required);
                }
                AppUpdate::DestDriveAvailable(available) => {
                    self.available_space = Some(available);
                }
                AppUpdate::StepStatusChanged => {
                    // Force UI update
                }
                AppUpdate::Log(log) => {
                    self.add_log(log);
                }
                _ => {}
            }
        }

        egui::CentralPanel::default().show(ctx, |ui| {
            egui::ScrollArea::vertical().show(ui, |ui| {
                draw_main(self, ui).unwrap();
            });
        });
    }
}

fn folder_selection(
    ui: &mut Ui,
    ctx: &Context,
    label: &str,
    tooltip: &str,
    dir_path: Option<PathBuf>,
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
            .as_ref()
            .map(|p| p.to_string_lossy().to_string())
            .unwrap_or_default();

        ui.horizontal(|ui| {
            let mut text_val = current_path_text.clone();
            let text_widget = TextEdit::singleline(&mut text_val).interactive(false);
            ui.add_sized([ui.available_width() - 120.0, 20.0], text_widget);

            if ui.button("📁 Select Folder").clicked() {
                let current = dir_path.clone();
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
                        ctx.set_outdir(new_dir);
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
            if let Some(path) = &dir_path {
                if let Err(e) = validate_fn(path) {
                    ui.colored_label(Color32::from_rgb(255, 100, 0), format!("⚠ {}", e));
                }
            }
        }
    });
}

fn folder_selection_required(
    ui: &mut Ui,
    ctx: &Context,
    label: &str,
    tooltip: &str,
    dir_path: PathBuf,
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
            let mut text_val = dir_path.to_str().unwrap_or_default().to_string();

            let text_widget = TextEdit::singleline(&mut text_val).interactive(false);
            ui.add_sized([ui.available_width() - 120.0, 20.0], text_widget);

            if ui.button("📁 Select Folder").clicked() {
                let current = dir_path.clone();
                let mut dialog = rfd::FileDialog::new();
                dialog = dialog.set_directory(current);
                if let Some(new_dir) = dialog.pick_folder() {
                    info!("Selected directory: {}", new_dir.display());
                    ctx.set_outdir(new_dir);
                }
            }
        });
    });
}

fn draw_status_banner(ui: &mut Ui, app: &App) {
    let mut has_banner = false;

    if let Some(err) = &app.error {
        ui.horizontal(|ui| {
            ui.label(
                RichText::new("✗ Error:")
                    .color(Color32::from_rgb(220, 0, 0))
                    .strong(),
            );
            ui.label(RichText::new(err).color(Color32::from_rgb(220, 0, 0)));
        });
        has_banner = true;
    } else if let Some(state) = &app.state {
        ui.horizontal(|ui| {
            ui.label(RichText::new("⏳").color(Color32::from_rgb(255, 165, 0)));
            ui.label(RichText::new(state).color(Color32::from_rgb(255, 165, 0)));
        });
        has_banner = true;
    }

    if let Some((desc, pct)) = &app.progress {
        let progress_bar = ProgressBar::new(*pct).text(desc);
        ui.add_sized([ui.available_width(), 20.0], progress_bar);
        has_banner = true;
    }

    if has_banner {
        ui.add_space(5.0);
    }
}

// Custom tracing layer that sends logs to the UI
pub struct UiLayer {
    pub tx: Sender<AppUpdate>,
}

impl<S> Layer<S> for UiLayer
where
    S: tracing::Subscriber,
{
    fn on_event(
        &self,
        event: &tracing::Event<'_>,
        _ctx: tracing_subscriber::layer::Context<'_, S>,
    ) {
        use tracing::field::Visit;

        struct MessageVisitor {
            message: String,
        }

        impl Visit for MessageVisitor {
            fn record_debug(&mut self, field: &tracing::field::Field, value: &dyn std::fmt::Debug) {
                if field.name() == "message" {
                    self.message = format!("{:?}", value);
                    // Remove surrounding quotes from debug format
                    if self.message.starts_with('"') && self.message.ends_with('"') {
                        self.message = self.message[1..self.message.len() - 1].to_string();
                    }
                }
            }
        }

        let mut visitor = MessageVisitor {
            message: String::new(),
        };
        event.record(&mut visitor);

        if !visitor.message.is_empty() {
            let level = event.metadata().level();
            let log_msg = format!("[{}] {}", level, visitor.message);
            let _ = self.tx.send(AppUpdate::Log(log_msg));
        }
    }
}
