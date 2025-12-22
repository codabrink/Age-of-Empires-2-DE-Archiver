use crate::{AppUpdate, config::Config};
use anyhow::{Result, bail};
use eframe::egui::Color32;
use std::{
    path::PathBuf,
    sync::{Arc, Mutex, mpsc::Sender},
};

pub struct Context {
    pub config: Config,
    pub tx: Sender<AppUpdate>,
    pub source_dir: Mutex<Option<PathBuf>>,
    pub outdir: Mutex<PathBuf>,
    pub current_task: Mutex<Option<Task>>,
    pub step_status: Mutex<[StepStatus; 4]>,
}

impl Context {
    pub fn outdir(&self) -> Result<PathBuf> {
        Ok(self.outdir.lock().unwrap().clone())
    }

    pub fn source_dir(&self) -> Result<Option<PathBuf>> {
        Ok(self.source_dir.lock().unwrap().clone())
    }

    pub fn set_step_status(&self, step: usize, status: StepStatus) {
        if let Ok(mut steps) = self.step_status.lock() {
            if step < steps.len() {
                steps[step] = status;
            }
        }

        let _ = self.tx.send(AppUpdate::StepStatusChanged);
    }

    pub fn current_task(&self) -> Option<Task> {
        self.current_task.lock().unwrap().clone()
    }
}

impl Context {
    pub fn set_task(self: &Arc<Self>, task: Task) -> Result<TaskReset> {
        let mut guard = self.current_task.lock().unwrap();
        if let Some(existing_task) = &*guard {
            bail!("Task already running: {existing_task:?}");
        };

        let reset = TaskReset::new(self.clone());
        *guard = Some(task);

        Ok(reset)
    }

    pub fn is_busy(&self) -> bool {
        self.current_task.lock().unwrap().is_some()
    }
}

#[derive(Debug, Clone)]
pub enum Task {
    Copy,
    Goldberg,
    Companion,
    Launcher,
}

pub struct TaskReset(Arc<Context>);
impl TaskReset {
    pub fn new(ctx: Arc<Context>) -> Self {
        Self(ctx)
    }
}
impl Drop for TaskReset {
    fn drop(&mut self) {
        *self.0.current_task.lock().unwrap() = None;
    }
}

#[derive(Debug, Clone, PartialEq)]
pub enum StepStatus {
    NotStarted,
    InProgress,
    Completed,
    Failed(String),
}

impl StepStatus {
    pub fn icon(&self) -> &str {
        match self {
            StepStatus::NotStarted => "⚪",
            StepStatus::InProgress => "⏳",
            StepStatus::Completed => "✓",
            StepStatus::Failed(_) => "✗",
        }
    }

    pub fn color(&self) -> Color32 {
        match self {
            StepStatus::NotStarted => Color32::GRAY,
            StepStatus::InProgress => Color32::from_rgb(255, 165, 0), // Orange
            StepStatus::Completed => Color32::from_rgb(0, 200, 0),    // Green
            StepStatus::Failed(_) => Color32::from_rgb(220, 0, 0),    // Red
        }
    }
}
