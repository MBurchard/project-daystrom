//! Profile launch values shared with the frontend and injected mod.
//!
//! Compatibility is checked by `scripts/profile-protocol.spec.ts`.

/// Environment variable used to pass the selected profile to the injected mod.
#[cfg(any(target_os = "macos", target_os = "windows"))]
pub(crate) const PROFILE_ENV_VAR: &str = "DAYSTROM_PROFILE";

/// Profile placeholder used while importing the first detected account.
pub(crate) const INITIAL_PROFILE_STEM: &str = "initial";

/// Profile placeholder used while creating a new local account profile.
pub(crate) const NEW_ACCOUNT_PROFILE_STEM: &str = "new_account";
