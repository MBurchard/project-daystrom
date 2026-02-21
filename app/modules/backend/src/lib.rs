#[cfg(debug_assertions)]
use tauri::Manager;

mod commands;
mod game;
mod logging;

use_log!("Startup");

/// Bootstrap and run the Tauri application.
///
/// Sets up logging, detects the STFC installation, checks entitlements, and
/// opens DevTools in debug builds.
#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tauri::Builder::default()
        .plugin(logging::build_plugin())
        .setup(|app| {
            let version = &app.package_info().version;
            log_info!("Skynet {version} initialised");

            match game::detect() {
                Some(info) => {
                    log_info!("STFC found: {}", info.executable.display());

                    let status = game::entitlements::check(&info.executable);
                    if status.all_granted() {
                        log_info!("Entitlements OK — mod injection ready");
                    } else {
                        let names: Vec<_> = status.missing.iter()
                            .map(|k| k.strip_prefix("com.apple.security.").unwrap_or(k))
                            .collect();
                        log_warn!("Missing entitlements: {}", names.join(", "));
                    }
                }
                None => log_warn!("STFC not found — game features will be unavailable"),
            }

            match game::find_mod_library(&app.handle()) {
                Some(path) => log_info!("Mod library found: {}", path.display()),
                None => log_warn!("Mod library not bundled — run pnpm build:mod"),
            }

            #[cfg(debug_assertions)]
            if std::env::var("SKYNET_DEVTOOLS").as_deref() != Ok("0") {
                let window = app.get_webview_window("main").unwrap();
                window.open_devtools();
                log_debug!("DevTools opened");
            }

            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            commands::get_game_status,
            commands::patch_entitlements,
            commands::launch_game,
        ])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
