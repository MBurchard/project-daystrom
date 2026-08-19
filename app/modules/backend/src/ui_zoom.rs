//! Backend-owned zoom control for the main Daystrom webview.

use std::sync::Mutex;

use serde::{Deserialize, Serialize};
use ts_rs::TS;

use crate::use_log;

use_log!("UiZoom");

const DEFAULT_ZOOM: f64 = 1.0;
const MIN_ZOOM: f64 = 0.5;
const MAX_ZOOM: f64 = 2.0;
const ZOOM_STEP: f64 = 0.1;

static CURRENT_ZOOM: Mutex<f64> = Mutex::new(DEFAULT_ZOOM);

/// User intent supported by the application zoom control.
#[derive(Clone, Copy, Debug, Deserialize, TS)]
#[serde(rename_all = "snake_case")]
#[ts(export, rename_all = "snake_case")]
pub enum UiZoomAction {
    /// Increase the zoom by one step.
    Increase,
    /// Decrease the zoom by one step.
    Decrease,
    /// Restore the default zoom factor.
    Reset,
}

/// Authoritative zoom state returned to the frontend after a change.
#[derive(Clone, Copy, Debug, PartialEq, Serialize, TS)]
#[ts(export)]
pub struct UiZoomState {
    /// Current webview zoom factor.
    pub factor: f64,
}

/// Apply the persisted zoom before the initially hidden window is shown.
pub fn initialize(window: &tauri::WebviewWindow) {
    let factor = normalize_zoom(crate::settings::get_ui_zoom().unwrap_or(DEFAULT_ZOOM));
    match window.set_zoom(factor) {
        Ok(()) => {
            *CURRENT_ZOOM.lock().unwrap() = factor;
            log_debug!("Initialized application zoom at {:.0}%", factor * 100.0);
        }
        Err(error) => log_warn!("Failed to initialize application zoom: {error}"),
    }
}

/// Apply, persist, and return one backend-validated zoom change.
#[tauri::command]
pub fn change_ui_zoom(window: tauri::WebviewWindow, action: UiZoomAction) -> Result<UiZoomState, String> {
    let mut current = CURRENT_ZOOM.lock().unwrap();
    let factor = next_zoom(*current, action);
    if factor != *current {
        window
            .set_zoom(factor)
            .map_err(|error| format!("failed to apply application zoom: {error}"))?;
        *current = factor;
        crate::settings::set_ui_zoom(factor);
        log_debug!("Application zoom changed to {:.0}%", factor * 100.0);
    }
    Ok(UiZoomState { factor })
}

/// Calculate the next normalized factor for one user action.
fn next_zoom(current: f64, action: UiZoomAction) -> f64 {
    match action {
        UiZoomAction::Increase => normalize_zoom(current + ZOOM_STEP),
        UiZoomAction::Decrease => normalize_zoom(current - ZOOM_STEP),
        UiZoomAction::Reset => DEFAULT_ZOOM,
    }
}

/// Clamp a zoom factor to the supported range and one decimal place.
fn normalize_zoom(factor: f64) -> f64 {
    if !factor.is_finite() {
        return DEFAULT_ZOOM;
    }
    let rounded = (factor * 10.0).round() / 10.0;
    rounded.clamp(MIN_ZOOM, MAX_ZOOM)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn normalizes_zoom_to_supported_steps_and_bounds() {
        assert_eq!(normalize_zoom(f64::NAN), DEFAULT_ZOOM);
        assert_eq!(normalize_zoom(0.1), MIN_ZOOM);
        assert_eq!(normalize_zoom(0.76), 0.8);
        assert_eq!(normalize_zoom(4.0), MAX_ZOOM);
    }

    #[test]
    fn applies_zoom_actions_without_exceeding_bounds() {
        assert_eq!(next_zoom(1.0, UiZoomAction::Increase), 1.1);
        assert_eq!(next_zoom(1.0, UiZoomAction::Decrease), 0.9);
        assert_eq!(next_zoom(1.7, UiZoomAction::Reset), DEFAULT_ZOOM);
        assert_eq!(next_zoom(MAX_ZOOM, UiZoomAction::Increase), MAX_ZOOM);
        assert_eq!(next_zoom(MIN_ZOOM, UiZoomAction::Decrease), MIN_ZOOM);
    }
}
