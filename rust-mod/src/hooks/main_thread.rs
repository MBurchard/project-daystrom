//! Main-thread executor for Unity/IL2CPP side effects.
//!
//! Settings arrive on the WebSocket thread, but Unity/IL2CPP objects and methods must be touched from the game's
//! main thread. Callers enqueue typed tasks with a snapshot of the required values; a main-thread hook drains and
//! coalesces the queue.

use std::mem;
use std::sync::{Mutex, TryLockError};

#[derive(Clone, Debug, PartialEq)]
pub(crate) enum MainThreadTask {
    UiScale { scale_pct: u32 },
    ChatFrame { auto_open_sidebar: bool },
    JobQueue { auto_expand_job_queue: bool },
    ToastBanner { disable_all: bool, disabled_types: Vec<String> },
    SystemZoom { system_zoom: u32, ship_names_visible: u32 },
    Hotkeys { trigger_main_action: Option<String> },
}

#[derive(Default)]
struct CoalescedTasks {
    ui_scale: Option<u32>,
    chat_frame: Option<bool>,
    job_queue: Option<bool>,
    toast_banner: Option<(bool, Vec<String>)>,
    system_zoom: Option<(u32, u32)>,
    hotkeys: Option<Option<String>>,
}

static TASKS: Mutex<Vec<MainThreadTask>> = Mutex::new(Vec::new());

/// Enqueue a task for the next main-thread drain.
pub(crate) fn enqueue(task: MainThreadTask) {
    TASKS.lock().unwrap_or_else(|e| e.into_inner()).push(task);
}

/// Run queued tasks from a known game main-thread hook.
pub fn drain_tasks() {
    let mut guard = match TASKS.try_lock() {
        Ok(guard) => guard,
        Err(TryLockError::Poisoned(e)) => e.into_inner(),
        Err(TryLockError::WouldBlock) => return,
    };
    if guard.is_empty() {
        return;
    }
    let tasks = mem::take(&mut *guard);
    drop(guard);

    let coalesced = coalesce(tasks);

    if let Some(scale_pct) = coalesced.ui_scale {
        super::ui_scale::apply_scale(scale_pct);
    }
    if let Some(auto_open_sidebar) = coalesced.chat_frame {
        super::chat_frame::on_settings_synced_value(auto_open_sidebar);
    }
    if let Some(auto_expand_job_queue) = coalesced.job_queue {
        super::job_queue::on_settings_synced_value(auto_expand_job_queue);
    }
    if let Some((disable_all, disabled_types)) = coalesced.toast_banner {
        super::toast_banner::on_settings_changed_value(disable_all, disabled_types);
    }
    if let Some((system_zoom, ship_names_visible)) = coalesced.system_zoom {
        super::system_zoom::on_settings_changed_value(system_zoom, ship_names_visible);
    }
    if let Some(trigger_main_action) = coalesced.hotkeys {
        super::hotkeys::on_shortcuts_changed_value(trigger_main_action);
    }
}

fn coalesce(tasks: Vec<MainThreadTask>) -> CoalescedTasks {
    let mut coalesced = CoalescedTasks::default();
    for task in tasks {
        match task {
            MainThreadTask::UiScale { scale_pct } => coalesced.ui_scale = Some(scale_pct),
            MainThreadTask::ChatFrame { auto_open_sidebar } => {
                coalesced.chat_frame = Some(auto_open_sidebar);
            }
            MainThreadTask::JobQueue { auto_expand_job_queue } => {
                coalesced.job_queue = Some(auto_expand_job_queue);
            }
            MainThreadTask::ToastBanner { disable_all, disabled_types } => {
                coalesced.toast_banner = Some((disable_all, disabled_types));
            }
            MainThreadTask::SystemZoom { system_zoom, ship_names_visible } => {
                coalesced.system_zoom = Some((system_zoom, ship_names_visible));
            }
            MainThreadTask::Hotkeys { trigger_main_action } => {
                coalesced.hotkeys = Some(trigger_main_action);
            }
        }
    }
    coalesced
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn coalesce_keeps_latest_task_per_feature() {
        let tasks = vec![
            MainThreadTask::UiScale { scale_pct: 110 },
            MainThreadTask::UiScale { scale_pct: 125 },
            MainThreadTask::SystemZoom {
                system_zoom: 1500,
                ship_names_visible: 1800,
            },
            MainThreadTask::SystemZoom {
                system_zoom: 2200,
                ship_names_visible: 2600,
            },
            MainThreadTask::Hotkeys {
                trigger_main_action: Some("F2".to_string()),
            },
            MainThreadTask::Hotkeys { trigger_main_action: None },
        ];

        let coalesced = coalesce(tasks);

        assert_eq!(coalesced.ui_scale, Some(125));
        assert_eq!(coalesced.system_zoom, Some((2200, 2600)));
        assert_eq!(coalesced.hotkeys, Some(None));
    }
}
