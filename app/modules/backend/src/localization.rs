//! Native strings that cannot be rendered through the Vue translation layer.

use crate::settings::{self, AppLanguage};

/// Return the native label for showing the main window.
pub fn show_window() -> &'static str {
    match settings::active_app_language() {
        AppLanguage::En => "Show Window",
        AppLanguage::De => "Fenster anzeigen",
        AppLanguage::Tlh => "Qorwagh yI'ang",
    }
}

/// Return the native label for quitting Daystrom.
pub fn quit() -> &'static str {
    match settings::active_app_language() {
        AppLanguage::En => "Quit",
        AppLanguage::De => "Beenden",
        AppLanguage::Tlh => "yImej",
    }
}

/// Return the native title shown when a Daystrom-owned process blocks quitting.
pub fn still_running_title() -> &'static str {
    match settings::active_app_language() {
        AppLanguage::En => "Still Running",
        AppLanguage::De => "Läuft noch",
        AppLanguage::Tlh => "QaptaH",
    }
}

/// Select the native quit warning for the processes that are still running.
pub fn quit_blocked_message(launcher_running: bool, game_running: bool) -> Option<&'static str> {
    match (settings::active_app_language(), launcher_running, game_running) {
        (AppLanguage::En, true, true) => Some(
            "The launcher and the game are still running.\n\
             Daystrom has been minimized to the tray instead.",
        ),
        (AppLanguage::En, true, false) => Some(
            "The launcher is still running.\n\
             Daystrom has been minimized to the tray instead.",
        ),
        (AppLanguage::En, false, true) => Some(
            "The game is still running.\n\
             Daystrom has been minimized to the tray instead.",
        ),
        (AppLanguage::De, true, true) => Some(
            "Der Launcher und das Spiel laufen noch.\n\
             Daystrom wurde stattdessen in den Infobereich minimiert.",
        ),
        (AppLanguage::De, true, false) => Some(
            "Der Launcher läuft noch.\n\
             Daystrom wurde stattdessen in den Infobereich minimiert.",
        ),
        (AppLanguage::De, false, true) => Some(
            "Das Spiel läuft noch.\n\
             Daystrom wurde stattdessen in den Infobereich minimiert.",
        ),
        (AppLanguage::Tlh, true, true) => Some(
            "Launcher Quj je QaptaH.\n\
             System trayDaq Daystrom So'lu'.",
        ),
        (AppLanguage::Tlh, true, false) => Some(
            "Launcher QaptaH.\n\
             System trayDaq Daystrom So'lu'.",
        ),
        (AppLanguage::Tlh, false, true) => Some(
            "Quj QaptaH.\n\
             System trayDaq Daystrom So'lu'.",
        ),
        (_, false, false) => None,
    }
}

/// Return the native title for minimize-to-tray hints.
pub fn minimized_title() -> &'static str {
    match settings::active_app_language() {
        AppLanguage::En => "Minimized to Tray",
        AppLanguage::De => "In den Infobereich minimiert",
        AppLanguage::Tlh => "System trayDaq So'lu'",
    }
}

/// Return the native dialogue body for the first minimize-to-tray hint.
pub fn minimized_dialogue_body() -> &'static str {
    match settings::active_app_language() {
        AppLanguage::En => {
            "Project Daystrom will continue running in the background.\n\
             Click the tray icon to reopen the window."
        }
        AppLanguage::De => {
            "Project Daystrom läuft im Hintergrund weiter.\n\
             Klicke auf das Symbol im Infobereich, um das Fenster erneut zu öffnen."
        }
        AppLanguage::Tlh => {
            "'emDaq QaptaH Project Daystrom.\n\
             Qorwagh 'angqa'meH system tray Degh yIwIv."
        }
    }
}

/// Return the native notification body for later minimize-to-tray hints.
pub fn minimized_notification_body() -> &'static str {
    match settings::active_app_language() {
        AppLanguage::En => "Project Daystrom is still running. Click the tray icon to reopen.",
        AppLanguage::De => "Project Daystrom läuft weiter. Klicke zum erneuten Öffnen auf das Symbol im Infobereich.",
        AppLanguage::Tlh => "QaptaH Project Daystrom. Qorwagh 'angqa'meH system tray Degh yIwIv.",
    }
}

/// Return the native title for the Windows mod-removal confirmation.
#[cfg(target_os = "windows")]
pub fn remove_mod_title() -> &'static str {
    match settings::active_app_language() {
        AppLanguage::En => "Remove Mod",
        AppLanguage::De => "Mod entfernen",
        AppLanguage::Tlh => "mod yIteq",
    }
}

/// Return the native body for the Windows mod-removal confirmation.
#[cfg(target_os = "windows")]
pub fn remove_mod_body() -> &'static str {
    match settings::active_app_language() {
        AppLanguage::En => {
            "Remove the Daystrom Mod?\n\n\
             After removal, the game can only be launched through the Scopely Launcher."
        }
        AppLanguage::De => {
            "Den Daystrom-Mod entfernen?\n\n\
             Danach kann das Spiel nur noch über den Scopely Launcher gestartet werden."
        }
        AppLanguage::Tlh => {
            "Daystrom mod dateq'a'?\n\n\
             ghIq Scopely Launcher neH lo'taHvIS Quj taghlaHlu'."
        }
    }
}

/// Return the native confirmation label for removing the mod.
#[cfg(target_os = "windows")]
pub fn remove() -> &'static str {
    match settings::active_app_language() {
        AppLanguage::En => "Remove",
        AppLanguage::De => "Entfernen",
        AppLanguage::Tlh => "yIteq",
    }
}

/// Return the native cancellation label.
#[cfg(target_os = "windows")]
pub fn cancel() -> &'static str {
    match settings::active_app_language() {
        AppLanguage::En => "Cancel",
        AppLanguage::De => "Abbrechen",
        AppLanguage::Tlh => "yIqIl",
    }
}

/// Return the native title for an available STFC update.
pub fn game_update_title() -> &'static str {
    match settings::active_app_language() {
        AppLanguage::En => "STFC update available",
        AppLanguage::De => "STFC-Update verfügbar",
        AppLanguage::Tlh => "STFC chu'moHmeH De' tu'lu'",
    }
}

/// Build the native body for an available STFC update.
pub fn game_update_body(version: u32) -> String {
    match settings::active_app_language() {
        AppLanguage::En => {
            format!("Version {version} is available. Close the game and open Daystrom to start the update.")
        }
        AppLanguage::De => {
            format!("Version {version} ist verfügbar. Schließe das Spiel und öffne Daystrom, um das Update zu starten.")
        }
        AppLanguage::Tlh => {
            format!("Version {version} tu'lu'. chu'moHmeH Quj yISoQmoH 'ej Daystrom yIpoSmoH.")
        }
    }
}

/// Return the native title for an available Daystrom update.
pub fn daystrom_update_title() -> &'static str {
    match settings::active_app_language() {
        AppLanguage::En => "Project Daystrom update available",
        AppLanguage::De => "Project-Daystrom-Update verfügbar",
        AppLanguage::Tlh => "Project Daystrom chu'moHmeH De' tu'lu'",
    }
}

/// Build the native body for an available Daystrom update.
pub fn daystrom_update_body(version: &str) -> String {
    match settings::active_app_language() {
        AppLanguage::En => format!("Version {version} is ready. Open Daystrom to review the update."),
        AppLanguage::De => {
            format!("Version {version} ist bereit. Öffne Daystrom, um das Update anzusehen.")
        }
        AppLanguage::Tlh => {
            format!("Version {version} SuqlaHlu'. chu'moHmeH Daystrom yIpoSmoH.")
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn quit_message_is_absent_when_nothing_is_running() {
        assert!(quit_blocked_message(false, false).is_none());
    }
}
