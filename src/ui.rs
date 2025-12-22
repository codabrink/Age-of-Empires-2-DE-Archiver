use crate::{App, AppUpdate, ctx::StepStatus, spawn_run_all_steps, utils::validate_aoe2_source};
use anyhow::Result;
use eframe::egui::{self, Button, Color32, ProgressBar, RichText, TextEdit, Ui};
use std::{
    path::{Path, PathBuf},
    sync::{Mutex, mpsc::Sender},
};
use tracing::info;
use tracing_subscriber::Layer;

fn draw_main(app: &mut App, ui: &mut Ui) -> Result<()> {
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
        &app.ctx.source_dir,
        Some(validate_aoe2_source),
    );
    ui.add_space(5.0);

    folder_selection_required(
        ui,
        "Destination Directory",
        "Select where you want to create the archived copy of the game",
        &mut app.ctx.outdir.lock().unwrap(),
    );
    ui.add_space(10.0);

    ui.separator();
    ui.add_space(10.0);

    let source_exists = app.ctx.source_dir.lock().unwrap().is_some();

    ui.separator();
    ui.add_space(10.0);

    // Run All button
    ui.horizontal(|ui| {
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
    });

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

    Ok(())
}

impl eframe::App for App {
    fn update(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        while let Ok(state) = self.update_rx.try_recv() {
            match state {
                AppUpdate::Progress(progress) => self.progress = progress,
                AppUpdate::DiskSpaceInfo(required, available) => {
                    self.disk_space_info = Some((required, available));
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
                draw_main(self, ui);
            });
        });
    }
}

fn folder_selection(
    ui: &mut Ui,
    label: &str,
    tooltip: &str,
    dir_path: &Mutex<Option<PathBuf>>,
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

fn folder_selection_required(ui: &mut Ui, label: &str, tooltip: &str, dir_path: &mut PathBuf) {
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
                    *dir_path = new_dir;
                }
            }
        });
    });
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
