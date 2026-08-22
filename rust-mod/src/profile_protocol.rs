//! Profile launch values shared with the Daystrom app.
//!
//! Compatibility is checked by `scripts/profile-protocol.spec.ts`.

/// Environment variable used to receive the selected profile from Daystrom.
pub(crate) const PROFILE_ENV_VAR: &str = "DAYSTROM_PROFILE";

/// Profile placeholder used while importing the first detected account.
pub(crate) const INITIAL_PROFILE_STEM: &str = "initial";

/// Profile placeholder used while creating a new local account profile.
pub(crate) const NEW_ACCOUNT_PROFILE_STEM: &str = "new_account";
