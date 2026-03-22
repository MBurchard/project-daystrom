use log::debug;

pub mod il2cpp_init;
mod player_prefs;
mod user_profile;

/// Install all game hooks after IL2CPP has been initialised.
///
/// Called from the `il2cpp_init` hook callback. Each hook logs its own success or failure;
/// a failed hook never prevents other hooks from being installed.
pub fn install_all_hooks() {
    let Some(api) = il2cpp_init::IL2CPP_API.get() else {
        return;
    };

    user_profile::install(api);
    player_prefs::install(api);

    debug!(target: "HookEngine", "Hook installation complete");
}
