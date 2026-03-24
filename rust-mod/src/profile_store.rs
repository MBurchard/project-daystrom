//! Persistent profile storage for Multi-Account support.
//!
//! Intercepts all PlayerPrefs access and stores values in a TOML file
//! (e.g. `106_Nabor.toml`) inside the Daystrom app data directory. All keys
//! are routed to the store; the Registry/plist is only consulted when a value
//! has not been captured yet (`None`). Once captured, the store is the single
//! source of truth.
//!
//! The filename is derived from `server_instance_id` and `social_username`.
//! On first start (import from Registry), data is collected in RAM until both
//! values are known, then the file is written.

use std::collections::BTreeMap;
use std::fs;
use std::path::PathBuf;
use std::sync::Mutex;
use std::time::{Duration, Instant};

use log::{debug, info, warn};
use serde::{Deserialize, Serialize};

use crate::TAURI_IDENTIFIER;

/// How often the store flushes to disk (when dirty).
const FLUSH_INTERVAL: Duration = Duration::from_secs(10);

// ---- TOML schema ----------------------------------------------------------

/// Root structure of the profile TOML file.
#[derive(Debug, Default, Serialize, Deserialize)]
struct ProfileData {
    #[serde(default)]
    profil: ProfilSection,
    #[serde(default, skip_serializing_if = "AuthSection::is_empty")]
    auth: AuthSection,
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    player: BTreeMap<String, toml::Value>,
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    factions: BTreeMap<String, String>,
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    chat: BTreeMap<String, toml::Value>,
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    misc: BTreeMap<String, toml::Value>,
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    cache: BTreeMap<String, toml::Value>,
}

/// The `[profil]` section: account identity and display info.
///
/// All fields are `Option` so we can distinguish "never set" (`None`) from
/// "set to empty" (`Some("")`). `None` means the hook should fall through to
/// the Registry to capture the real value.
#[derive(Debug, Default, Serialize, Deserialize)]
struct ProfilSection {
    #[serde(default, rename = "type", skip_serializing_if = "Option::is_none")]
    profile_type: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    user_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    social_username: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    player_level: Option<i32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    server_instance_id: Option<i32>,
}

/// The `[auth]` section: login credentials and authentication state.
#[derive(Debug, Default, Serialize, Deserialize)]
struct AuthSection {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    selected_authentication: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    master_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    primary_source: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    sources_list: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    adhoc_username: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    adhoc_password: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    scopely_id_username: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    scopely_id_password: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    s_adhoc_username: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    s_adhoc_password: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    s_scopely_id_username: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    s_scopely_id_password: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    login_username: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    login_password: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    s_login_username: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    s_login_password: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    login_allow_association: Option<i32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    scopely_id_allow_association: Option<i32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    current_version: Option<i32>,
}

impl AuthSection {
    /// Check if all fields are `None` (for `skip_serializing_if`).
    fn is_empty(&self) -> bool {
        self.selected_authentication.is_none()
            && self.master_id.is_none()
            && self.primary_source.is_none()
            && self.sources_list.is_none()
            && self.adhoc_username.is_none()
            && self.adhoc_password.is_none()
            && self.scopely_id_username.is_none()
            && self.scopely_id_password.is_none()
            && self.s_adhoc_username.is_none()
            && self.s_adhoc_password.is_none()
            && self.s_scopely_id_username.is_none()
            && self.s_scopely_id_password.is_none()
            && self.login_username.is_none()
            && self.login_password.is_none()
            && self.s_login_username.is_none()
            && self.s_login_password.is_none()
            && self.login_allow_association.is_none()
            && self.scopely_id_allow_association.is_none()
            && self.current_version.is_none()
    }
}

// ---- Key routing ----------------------------------------------------------

/// Determines which TOML section a PlayerPrefs key belongs to.
///
/// Every key is routed somewhere. `[profil]` and `[auth]` have named fields,
/// everything else falls through to `[misc]` as a key-value map.
enum KeyRoute<'a> {
    /// `[profil]` section, with the field name.
    Profil(&'a str),
    /// `[auth]` section, with the field name.
    Auth(&'a str),
    /// `[chat]` section, stored under the original key.
    Chat,
    /// `[misc]` section, stored under the original key.
    Misc,
    /// `[cache]` section, stored under the original key. Serialised last.
    Cache,
}

/// Route a PlayerPrefs key to its TOML section.
fn route_key(key: &str) -> KeyRoute<'_> {
    match key {
        // [profil]
        "ScopelyProfile.UserId" | "Scopely.Attribution.UserId"
        | "known_accounts" => KeyRoute::Profil("user_id"),
        "social_username" => KeyRoute::Profil("social_username"),
        "player_level" => KeyRoute::Profil("player_level"),
        "accounts/3/server_instance_id" => KeyRoute::Profil("server_instance_id"),
        // [auth]
        "selected_authentication" => KeyRoute::Auth("selected_authentication"),
        "accounts/current_version" => KeyRoute::Auth("current_version"),
        "accounts/3/master_id" => KeyRoute::Auth("master_id"),
        "accounts/3/primary_source" => KeyRoute::Auth("primary_source"),
        "accounts/3/sources_list" => KeyRoute::Auth("sources_list"),
        "accounts/3/adhoc_username" => KeyRoute::Auth("adhoc_username"),
        "accounts/3/adhoc_password" => KeyRoute::Auth("adhoc_password"),
        "accounts/3/scopely_id/username" => KeyRoute::Auth("scopely_id_username"),
        "accounts/3/scopely_id/password" => KeyRoute::Auth("scopely_id_password"),
        "accounts/3/scopely_id/allow_association" => KeyRoute::Auth("scopely_id_allow_association"),
        "saccounts/3/adhoc_username" => KeyRoute::Auth("s_adhoc_username"),
        "saccounts/3/adhoc_password" => KeyRoute::Auth("s_adhoc_password"),
        "saccounts/3/scopely_id/username" => KeyRoute::Auth("s_scopely_id_username"),
        "saccounts/3/scopely_id/password" => KeyRoute::Auth("s_scopely_id_password"),
        "accounts/3/login/username" => KeyRoute::Auth("login_username"),
        "accounts/3/login/password" => KeyRoute::Auth("login_password"),
        "accounts/3/login/allow_association" => KeyRoute::Auth("login_allow_association"),
        "saccounts/3/login/username" => KeyRoute::Auth("s_login_username"),
        "saccounts/3/login/password" => KeyRoute::Auth("s_login_password"),
        // [chat]
        "chat_tabpreference" | "recent_emojis" => KeyRoute::Chat,
        // [cache]
        "DownloadCacheHistoryV2" => KeyRoute::Cache,
        // [misc] (everything else)
        _ => KeyRoute::Misc,
    }
}

/// A value from the player section, either a plain string (factions) or a TOML value (player).
enum PlayerValue<'a> {
    Str(&'a str),
    Toml(&'a toml::Value),
}

// ---- Store state ----------------------------------------------------------

/// Profile mode, determined by the `DAYSTROM_PROFILE` environment variable.
#[derive(Clone, Debug, PartialEq)]
enum ProfileMode {
    /// No env variable: first start, import from Registry.
    Import,
    /// Known profile (e.g. `106_Nabor`): load from TOML, no Registry.
    Known(String),
    /// New account: everything is None, no Registry, no TOML until name is known.
    NewAccount,
}

/// In-memory profile store with dirty tracking and periodic flush.
struct StoreState {
    data: ProfileData,
    dirty: bool,
    last_flush: Instant,
    mode: ProfileMode,
    /// Current profile stem (e.g. "106_Nabor"). Used to detect renames.
    current_stem: Option<String>,
}

impl StoreState {
    /// Whether a key can be correctly routed right now.
    ///
    /// Phase 1 (no `user_id`): only keys with an explicit match in
    /// `route_key()` are routable. Keys that fall through to the `Misc`
    /// catch-all might be uid-prefixed and we cannot tell yet.
    /// Phase 2 (`user_id` known): everything is routable.
    fn can_route(&self, key: &str) -> bool {
        if self.data.profil.user_id.is_some() {
            return true;
        }
        !matches!(route_key(key), KeyRoute::Misc)
    }

    /// Check if a key is a user-ID-prefixed chat key (e.g. `{uid}chatHist`).
    ///
    /// These keys use the user ID directly concatenated with the suffix (no colon
    /// separator), unlike the other user-prefixed keys.
    fn is_chat_key(&self, key: &str) -> bool {
        let Some(uid) = self.data.profil.user_id.as_deref() else { return false };
        if uid.is_empty() { return false; }
        key.strip_prefix(uid).is_some_and(|suffix| suffix == "chatHist")
    }

    /// Strip the user-ID prefix from a PlayerPrefs key, if present.
    ///
    /// Keys like `i9170ba...:factions_federation_pips_hide_time` have the user ID
    /// (from `[profil].user_id`) as prefix. Returns the part after the `:` if the
    /// prefix matches, or `None` if it doesn't.
    fn strip_user_prefix<'a>(&self, key: &'a str) -> Option<&'a str> {
        let uid = self.data.profil.user_id.as_deref()?;
        if uid.is_empty() {
            return None;
        }
        key.strip_prefix(uid)?.strip_prefix(':')
    }

    /// Route a user-ID-prefixed key to its TOML section for reading.
    ///
    /// `factions_*` keys go to `[factions]`, everything else to `[player]`.
    fn get_player_value(&self, suffix: &str) -> Option<PlayerValue<'_>> {
        if let Some(factions_key) = suffix.strip_prefix("factions_") {
            return self.data.factions.get(factions_key).map(|v| PlayerValue::Str(v.as_str()));
        }
        self.data.player.get(suffix).map(PlayerValue::Toml)
    }

    /// Store a string value for a user-ID-prefixed key.
    fn put_player_string(&mut self, suffix: &str, value: &str) -> bool {
        if let Some(factions_key) = suffix.strip_prefix("factions_") {
            if self.data.factions.get(factions_key).is_some_and(|v| v == value) {
                return false;
            }
            self.data.factions.insert(factions_key.to_string(), value.to_string());
            self.dirty = true;
            return true;
        }
        let new_val = toml::Value::String(value.to_string());
        if self.data.player.get(suffix).is_some_and(|v| *v == new_val) {
            return false;
        }
        self.data.player.insert(suffix.to_string(), new_val);
        self.dirty = true;
        true
    }

    /// Store an int value for a user-ID-prefixed key.
    fn put_player_int(&mut self, suffix: &str, value: i32) -> bool {
        let new_val = toml::Value::Integer(value as i64);
        if self.data.player.get(suffix).is_some_and(|v| *v == new_val) {
            return false;
        }
        self.data.player.insert(suffix.to_string(), new_val);
        self.dirty = true;
        true
    }

    /// Resolve a mutable reference to an auth string field by name.
    fn auth_field_mut(&mut self, field: &str) -> Option<&mut Option<String>> {
        match field {
            "selected_authentication" => Some(&mut self.data.auth.selected_authentication),
            "master_id" => Some(&mut self.data.auth.master_id),
            "primary_source" => Some(&mut self.data.auth.primary_source),
            "sources_list" => Some(&mut self.data.auth.sources_list),
            "adhoc_username" => Some(&mut self.data.auth.adhoc_username),
            "adhoc_password" => Some(&mut self.data.auth.adhoc_password),
            "scopely_id_username" => Some(&mut self.data.auth.scopely_id_username),
            "scopely_id_password" => Some(&mut self.data.auth.scopely_id_password),
            "s_adhoc_username" => Some(&mut self.data.auth.s_adhoc_username),
            "s_adhoc_password" => Some(&mut self.data.auth.s_adhoc_password),
            "s_scopely_id_username" => Some(&mut self.data.auth.s_scopely_id_username),
            "s_scopely_id_password" => Some(&mut self.data.auth.s_scopely_id_password),
            "login_username" => Some(&mut self.data.auth.login_username),
            "login_password" => Some(&mut self.data.auth.login_password),
            "s_login_username" => Some(&mut self.data.auth.s_login_username),
            "s_login_password" => Some(&mut self.data.auth.s_login_password),
            _ => None,
        }
    }

    /// Resolve an immutable reference to an auth string field by name.
    fn auth_field(&self, field: &str) -> Option<&Option<String>> {
        match field {
            "selected_authentication" => Some(&self.data.auth.selected_authentication),
            "master_id" => Some(&self.data.auth.master_id),
            "primary_source" => Some(&self.data.auth.primary_source),
            "sources_list" => Some(&self.data.auth.sources_list),
            "adhoc_username" => Some(&self.data.auth.adhoc_username),
            "adhoc_password" => Some(&self.data.auth.adhoc_password),
            "scopely_id_username" => Some(&self.data.auth.scopely_id_username),
            "scopely_id_password" => Some(&self.data.auth.scopely_id_password),
            "s_adhoc_username" => Some(&self.data.auth.s_adhoc_username),
            "s_adhoc_password" => Some(&self.data.auth.s_adhoc_password),
            "s_scopely_id_username" => Some(&self.data.auth.s_scopely_id_username),
            "s_scopely_id_password" => Some(&self.data.auth.s_scopely_id_password),
            "login_username" => Some(&self.data.auth.login_username),
            "login_password" => Some(&self.data.auth.login_password),
            "s_login_username" => Some(&self.data.auth.s_login_username),
            "s_login_password" => Some(&self.data.auth.s_login_password),
            _ => None,
        }
    }

    /// Resolve a profil string field by name.
    fn profil_field(&self, field: &str) -> Option<&Option<String>> {
        match field {
            "user_id" => Some(&self.data.profil.user_id),
            "social_username" => Some(&self.data.profil.social_username),
            _ => None,
        }
    }

    /// Resolve a mutable profil string field by name.
    fn profil_field_mut(&mut self, field: &str) -> Option<&mut Option<String>> {
        match field {
            "user_id" => Some(&mut self.data.profil.user_id),
            "social_username" => Some(&mut self.data.profil.social_username),
            _ => None,
        }
    }

    // ---- put methods ------------------------------------------------------

    /// Store a string value. Returns `true` if changed.
    fn put(&mut self, key: &str, value: &str) -> bool {
        if self.is_chat_key(key) {
            return self.put_chat(key, toml::Value::String(value.to_string()));
        }
        if let Some(suffix) = self.strip_user_prefix(key) {
            return self.put_player_string(suffix, value);
        }

        match route_key(key) {
            KeyRoute::Profil(field) => {
                let Some(current) = self.profil_field_mut(field) else {
                    return false; // Int-only profil field
                };
                let new_val = Some(value.to_string());
                if *current == new_val {
                    return false;
                }
                *current = new_val;
                self.dirty = true;
                true
            }
            KeyRoute::Auth(field) => {
                let Some(current) = self.auth_field_mut(field) else {
                    // Int-only field written as String (e.g. allow_association = "").
                    return true; // known, nothing to store
                };
                let new_val = Some(value.to_string());
                if *current == new_val {
                    return false;
                }
                *current = new_val;
                self.dirty = true;
                true
            }
            KeyRoute::Chat => self.put_chat(key, toml::Value::String(value.to_string())),
            KeyRoute::Misc => self.put_misc(key, toml::Value::String(value.to_string())),
            KeyRoute::Cache => self.put_cache(key, toml::Value::String(value.to_string())),
        }
    }

    /// Store an integer value. Returns `true` if changed.
    fn put_int(&mut self, key: &str, value: i32) -> bool {
        if let Some(suffix) = self.strip_user_prefix(key) {
            return self.put_player_int(suffix, value);
        }
        match route_key(key) {
            KeyRoute::Profil("player_level") => {
                if self.data.profil.player_level == Some(value) { return false; }
                self.data.profil.player_level = Some(value);
                self.dirty = true;
                true
            }
            KeyRoute::Profil("server_instance_id") => {
                if self.data.profil.server_instance_id == Some(value) { return false; }
                self.data.profil.server_instance_id = Some(value);
                self.dirty = true;
                true
            }
            KeyRoute::Auth("current_version") => {
                if self.data.auth.current_version == Some(value) { return false; }
                self.data.auth.current_version = Some(value);
                self.dirty = true;
                true
            }
            KeyRoute::Auth("scopely_id_allow_association") => {
                if self.data.auth.scopely_id_allow_association == Some(value) { return false; }
                self.data.auth.scopely_id_allow_association = Some(value);
                self.dirty = true;
                true
            }
            KeyRoute::Auth("login_allow_association") => {
                if self.data.auth.login_allow_association == Some(value) { return false; }
                self.data.auth.login_allow_association = Some(value);
                self.dirty = true;
                true
            }
            KeyRoute::Chat => self.put_chat(key, toml::Value::Integer(value as i64)),
            KeyRoute::Cache => self.put_cache(key, toml::Value::Integer(value as i64)),
            KeyRoute::Profil(_) | KeyRoute::Auth(_) | KeyRoute::Misc => {
                self.put_misc(key, toml::Value::Integer(value as i64))
            }
        }
    }

    /// Store a float value. Returns `true` if changed.
    fn put_float(&mut self, key: &str, value: f32) -> bool {
        if let Some(suffix) = self.strip_user_prefix(key) {
            let new_val = toml::Value::Float(value as f64);
            if self.data.player.get(suffix).is_some_and(|v| *v == new_val) {
                return false;
            }
            self.data.player.insert(suffix.to_string(), new_val);
            self.dirty = true;
            return true;
        }
        match route_key(key) {
            KeyRoute::Chat => self.put_chat(key, toml::Value::Float(value as f64)),
            KeyRoute::Cache => self.put_cache(key, toml::Value::Float(value as f64)),
            _ => self.put_misc(key, toml::Value::Float(value as f64)),
        }
    }

    /// Insert a value into `[misc]`, returning `true` if it changed.
    fn put_misc(&mut self, key: &str, value: toml::Value) -> bool {
        if self.data.misc.get(key).is_some_and(|v| *v == value) {
            return false;
        }
        self.data.misc.insert(key.to_string(), value);
        self.dirty = true;
        true
    }

    /// Insert a value into `[chat]`, returning `true` if it changed.
    fn put_chat(&mut self, key: &str, value: toml::Value) -> bool {
        if self.data.chat.get(key).is_some_and(|v| *v == value) {
            return false;
        }
        self.data.chat.insert(key.to_string(), value);
        self.dirty = true;
        true
    }

    /// Insert a value into `[cache]`, returning `true` if it changed.
    fn put_cache(&mut self, key: &str, value: toml::Value) -> bool {
        if self.data.cache.get(key).is_some_and(|v| *v == value) {
            return false;
        }
        self.data.cache.insert(key.to_string(), value);
        self.dirty = true;
        true
    }

    // ---- get methods ------------------------------------------------------

    /// Check whether a key has a stored value (any type).
    fn contains(&self, key: &str) -> bool {
        if self.is_chat_key(key) {
            return self.data.chat.contains_key(key);
        }
        if let Some(suffix) = self.strip_user_prefix(key) {
            return self.get_player_value(suffix).is_some();
        }

        match route_key(key) {
            KeyRoute::Profil(field) => match field {
                "player_level" => self.data.profil.player_level.is_some(),
                "server_instance_id" => self.data.profil.server_instance_id.is_some(),
                _ => self.profil_field(field).is_some_and(|v| v.is_some()),
            },
            KeyRoute::Auth(field) => match field {
                "current_version" => self.data.auth.current_version.is_some(),
                "scopely_id_allow_association" => self.data.auth.scopely_id_allow_association.is_some(),
                "login_allow_association" => self.data.auth.login_allow_association.is_some(),
                _ => self.auth_field(field).is_some_and(|v| v.is_some()),
            },
            KeyRoute::Chat => self.data.chat.contains_key(key),
            KeyRoute::Misc => self.data.misc.contains_key(key),
            KeyRoute::Cache => self.data.cache.contains_key(key),
        }
    }

    /// Look up a stored string value. Returns `None` if never set.
    fn get(&self, key: &str) -> Option<&str> {
        if self.is_chat_key(key) {
            return self.data.chat.get(key).and_then(|v| v.as_str());
        }
        if let Some(suffix) = self.strip_user_prefix(key) {
            return match self.get_player_value(suffix)? {
                PlayerValue::Str(s) => Some(s),
                PlayerValue::Toml(v) => v.as_str(),
            };
        }

        match route_key(key) {
            KeyRoute::Profil(field) => {
                self.profil_field(field)?.as_deref()
            }
            KeyRoute::Auth(field) => {
                // String fields: return stored value (including Some(""))
                if let Some(opt) = self.auth_field(field) {
                    return opt.as_deref();
                }
                // Int-only field read as String: return None (fall through to Registry).
                // The Int value is the real one; the Registry returns "" for the String read.
                None
            }
            KeyRoute::Chat => self.data.chat.get(key).and_then(|v| v.as_str()),
            KeyRoute::Misc => self.data.misc.get(key).and_then(|v| v.as_str()),
            KeyRoute::Cache => self.data.cache.get(key).and_then(|v| v.as_str()),
        }
    }

    /// Look up a stored integer value.
    fn get_int(&self, key: &str) -> Option<i32> {
        if let Some(suffix) = self.strip_user_prefix(key) {
            return self.data.player.get(suffix).and_then(|v| v.as_integer()).map(|v| v as i32);
        }
        match route_key(key) {
            KeyRoute::Profil("player_level") => self.data.profil.player_level,
            KeyRoute::Profil("server_instance_id") => self.data.profil.server_instance_id,
            KeyRoute::Auth("current_version") => self.data.auth.current_version,
            KeyRoute::Auth("scopely_id_allow_association") => self.data.auth.scopely_id_allow_association,
            KeyRoute::Auth("login_allow_association") => self.data.auth.login_allow_association,
            KeyRoute::Chat => self.data.chat.get(key).and_then(|v| v.as_integer()).map(|v| v as i32),
            KeyRoute::Misc => self.data.misc.get(key).and_then(|v| v.as_integer()).map(|v| v as i32),
            KeyRoute::Cache => self.data.cache.get(key).and_then(|v| v.as_integer()).map(|v| v as i32),
            _ => None,
        }
    }

    /// Look up a stored float value.
    fn get_float(&self, key: &str) -> Option<f32> {
        if let Some(suffix) = self.strip_user_prefix(key) {
            return self.data.player.get(suffix).and_then(|v| v.as_float()).map(|v| v as f32);
        }
        match route_key(key) {
            KeyRoute::Chat => self.data.chat.get(key).and_then(|v| v.as_float()).map(|v| v as f32),
            KeyRoute::Misc => self.data.misc.get(key).and_then(|v| v.as_float()).map(|v| v as f32),
            KeyRoute::Cache => self.data.cache.get(key).and_then(|v| v.as_float()).map(|v| v as f32),
            _ => None,
        }
    }

    // ---- delete -----------------------------------------------------------

    /// Remove a key from the store. Returns `true` if a value was removed.
    fn delete(&mut self, key: &str) -> bool {
        if let Some(suffix) = self.strip_user_prefix(key) {
            if let Some(factions_key) = suffix.strip_prefix("factions_") {
                return self.data.factions.remove(factions_key).is_some();
            }
            return self.data.player.remove(suffix).is_some();
        }

        if self.data.chat.remove(key).is_some() {
            return true;
        }
        if self.data.misc.remove(key).is_some() {
            return true;
        }
        if self.data.cache.remove(key).is_some() {
            return true;
        }

        match route_key(key) {
            KeyRoute::Profil(field) => {
                if let Some(f) = self.profil_field_mut(field) {
                    return f.take().is_some();
                }
                match field {
                    "player_level" => self.data.profil.player_level.take().is_some(),
                    "server_instance_id" => self.data.profil.server_instance_id.take().is_some(),
                    _ => false,
                }
            }
            KeyRoute::Auth(_) | KeyRoute::Chat | KeyRoute::Misc | KeyRoute::Cache => false,
        }
    }

    // ---- flush ------------------------------------------------------------

    /// Check whether we can flush (name + server must be known).
    fn can_flush(&self) -> bool {
        self.data.profil.social_username.is_some() && self.data.profil.server_instance_id.is_some()
    }

    /// Check whether a flush is due. Returns the serialised content and filename if so.
    ///
    /// The actual disk write happens OUTSIDE the mutex to avoid blocking.
    fn take_pending_flush(&mut self) -> Option<(String, String)> {
        if !self.dirty || self.last_flush.elapsed() < FLUSH_INTERVAL {
            return None;
        }
        if !self.can_flush() {
            return None; // waiting for server + name
        }

        let new_stem = profile_stem(
            self.data.profil.server_instance_id.unwrap_or(0),
            self.data.profil.social_username.as_deref().unwrap_or("unknown"),
        );

        // Detect rename: stem changed since last flush (or first flush)
        if self.current_stem.as_deref() != Some(new_stem.as_str()) {
            if let Some(old_stem) = &self.current_stem {
                rename_profile_file(old_stem, &new_stem);
            }
            crate::logging::rename_log(&new_stem);
            self.current_stem = Some(new_stem.clone());
        }

        match toml::to_string_pretty(&self.data) {
            Ok(content) => {
                self.dirty = false;
                self.last_flush = Instant::now();
                let filename = format!("{new_stem}.toml");
                Some((content, filename))
            }
            Err(e) => {
                warn!(target: "ProfileStore", "Failed to serialise profile: {e}");
                None
            }
        }
    }
}

/// Global store instance, initialised lazily on first access.
static STORE: Mutex<Option<StoreState>> = Mutex::new(None);

// ---- Profile path + filename ----------------------------------------------

/// Determine the directory for profile data.
///
/// Uses the same base as the Daystrom app settings:
/// - macOS: `~/Library/Application Support/{TAURI_IDENTIFIER}/`
/// - Windows: `{APPDATA}/{TAURI_IDENTIFIER}/`
fn profile_dir() -> Option<PathBuf> {
    let base = dirs::config_dir()?;
    Some(base.join(TAURI_IDENTIFIER))
}

/// Rename a profile TOML file on disk when the player name changes.
fn rename_profile_file(old_stem: &str, new_stem: &str) {
    let Some(dir) = profile_dir() else { return };
    let old_path = dir.join(format!("{old_stem}.toml"));
    let new_path = dir.join(format!("{new_stem}.toml"));
    if old_path.exists() && !new_path.exists() {
        if let Err(e) = fs::rename(&old_path, &new_path) {
            warn!(target: "ProfileStore", "Failed to rename {old_stem}.toml to {new_stem}.toml: {e}");
        } else {
            info!(target: "ProfileStore", "Profile renamed: {old_stem} -> {new_stem}");
        }
    }
}

/// Build a profile stem (filename without `.toml`) from server ID and player name.
///
/// Sanitises the name to ASCII-alphanumeric + underscore for safe filenames.
fn profile_stem(server_id: i32, name: &str) -> String {
    let safe_name: String = name
        .chars()
        .map(|c| if c.is_ascii_alphanumeric() || c == '_' { c } else { '_' })
        .collect();
    let safe_name = if safe_name.is_empty() { "unknown".to_string() } else { safe_name };
    format!("{server_id}_{safe_name}")
}

/// Find the first existing profile TOML in the profile directory.
///
/// Returns the path and parsed data if found.
fn find_existing_profile() -> Option<(PathBuf, ProfileData)> {
    let dir = profile_dir()?;
    let entries = fs::read_dir(&dir).ok()?;

    for entry in entries.flatten() {
        let path = entry.path();
        if path.extension().and_then(|e| e.to_str()) != Some("toml") {
            continue;
        }
        let name = path.file_stem()?.to_str()?;
        // Profile files have the format {server}_{name}
        if name.contains('_')
            && name.chars().next().is_some_and(|c| c.is_ascii_digit())
            && let Ok(content) = fs::read_to_string(&path)
            && let Ok(data) = toml::from_str::<ProfileData>(&content)
        {
            return Some((path, data));
        }
    }
    None
}

// ---- Initialisation -------------------------------------------------------

/// Get or initialise the store.
fn with_store<F, R>(f: F) -> Option<R>
where
    F: FnOnce(&mut StoreState) -> R,
{
    let mut guard = STORE.lock().unwrap_or_else(|e| e.into_inner());
    let state = guard.get_or_insert_with(|| {
        let mode = match std::env::var(crate::logging::PROFILE_ENV) {
            Ok(val) if val == "new_account" => ProfileMode::NewAccount,
            Ok(val) if val == "initial" => ProfileMode::Import,
            Ok(val) if !val.is_empty() => ProfileMode::Known(val),
            _ => ProfileMode::Import,
        };

        match &mode {
            ProfileMode::Known(stem) => {
                // Load the specific profile TOML
                let filename = format!("{stem}.toml");
                let path = profile_dir().map(|d| d.join(&filename));
                let data = path.as_ref().and_then(|p| {
                    fs::read_to_string(p).ok().and_then(|content| {
                        toml::from_str::<ProfileData>(&content).ok()
                    })
                }).unwrap_or_default();
                let key_count = data.misc.len() + data.chat.len() + data.cache.len() + data.factions.len() + data.player.len();
                let current_stem = Some(stem.clone());
                info!(target: "ProfileStore", "Loaded profile '{stem}' ({key_count} keys)");
                StoreState {
                    data,
                    dirty: false,
                    last_flush: Instant::now(),
                    mode: mode.clone(),
                    current_stem,
                }
            }
            ProfileMode::NewAccount => {
                info!(target: "ProfileStore", "New account mode: all values empty");
                StoreState {
                    data: ProfileData::default(),
                    dirty: false,
                    last_flush: Instant::now(),
                    mode,
                    current_stem: None,
                }
            }
            ProfileMode::Import => {
                // Try to load an existing profile, otherwise start fresh
                if let Some((path, mut data)) = find_existing_profile() {
                    let key_count = data.misc.len() + data.chat.len() + data.cache.len() + data.factions.len() + data.player.len();
                    data.profil.profile_type = Some("primary".to_string());
                    info!(target: "ProfileStore", "Loaded profile from {} ({key_count} keys)", path.display());
                    StoreState {
                        data,
                        dirty: true,
                        last_flush: Instant::now(),
                        mode,
                        current_stem: None,
                    }
                } else {
                    debug!(target: "ProfileStore", "No profile found, importing from Registry");
                    let mut data = ProfileData::default();
                    data.profil.profile_type = Some("primary".to_string());
                    StoreState {
                        data,
                        dirty: false,
                        last_flush: Instant::now(),
                        mode,
                        current_stem: None,
                    }
                }
            }
        }
    });
    Some(f(state))
}

// ---- Disk I/O (outside mutex) ---------------------------------------------

/// Write serialised content to disk. Called OUTSIDE the mutex.
fn flush_content(content: &str, filename: &str) {
    let Some(dir) = profile_dir() else { return };
    let _ = fs::create_dir_all(&dir);
    let path = dir.join(filename);
    match fs::write(&path, content) {
        Ok(()) => debug!(target: "ProfileStore", "Flushed profile to {}", path.display()),
        Err(e) => warn!(target: "ProfileStore", "Failed to write {}: {e}", path.display()),
    }
}

/// Execute a store operation and flush if needed.
fn store_and_flush<F, R>(f: F) -> Option<R>
where
    F: FnOnce(&mut StoreState) -> (R, Option<(String, String)>),
{
    let (result, pending) = with_store(|state| {
        let (result, _) = f(state);
        let pending = state.take_pending_flush();
        (result, pending)
    })?;

    if let Some((content, filename)) = pending {
        flush_content(&content, &filename);
    }
    Some(result)
}

// ---- Public API -----------------------------------------------------------

/// Record a PlayerPrefs string value (from GET or SET).
///
/// Returns `true` if the key was already known (suppress log).
pub fn record(key: &str, value: &str) -> bool {
    store_and_flush(|state| {
        if !state.can_route(key) {
            return (true, None);
        }
        let was_known = state.contains(key);
        state.put(key, value);
        (was_known, None)
    })
    .unwrap_or(false)
}

/// Look up a stored string value by PlayerPrefs key.
///
/// Returns `None` if the value was never set (hook should fall through to Registry).
pub fn get(key: &str) -> Option<String> {
    with_store(|state| state.get(key).map(|v| v.to_string()))
    .flatten()
}

/// Record a PlayerPrefs integer value (from GET_INT or SET_INT).
pub fn record_int(key: &str, value: i32) -> bool {
    store_and_flush(|state| {
        if !state.can_route(key) {
            return (true, None);
        }
        let was_known = state.contains(key);
        state.put_int(key, value);
        (was_known, None)
    })
    .unwrap_or(false)
}

/// Look up a stored integer value.
pub fn get_int(key: &str) -> Option<i32> {
    with_store(|state| state.get_int(key))
    .flatten()
}

/// Record a PlayerPrefs float value (from GET_FLOAT or SET_FLOAT).
pub fn record_float(key: &str, value: f32) -> bool {
    store_and_flush(|state| {
        if !state.can_route(key) {
            return (true, None);
        }
        let was_known = state.contains(key);
        state.put_float(key, value);
        (was_known, None)
    })
    .unwrap_or(false)
}

/// Look up a stored float value.
pub fn get_float(key: &str) -> Option<f32> {
    with_store(|state| state.get_float(key))
    .flatten()
}

/// Delete a key from the store.
pub fn delete(key: &str) {
    store_and_flush(|state| {
        let removed = state.delete(key);
        if removed {
            state.dirty = true;
        }
        ((), None)
    });
}

/// Whether a key is routed through the profile store.
///
/// In Phase 1 (no `user_id`), only keys with explicit routing rules are
/// intercepted. Keys that would fall through to `[misc]` pass transparently
/// to the original PlayerPrefs methods until `user_id` is known.
pub fn is_routed(key: &str) -> bool {
    with_store(|state| state.can_route(key)).unwrap_or(false)
}

/// Whether the store should block Registry fallthrough for unknown values.
///
/// In `NewAccount` and `Known` modes, unknown values must NOT fall through to
/// the Registry. Only in `Import` mode or for `primary` profiles do we allow it.
pub fn should_block_registry() -> bool {
    with_store(|state| {
        if state.mode == ProfileMode::Import {
            return false;
        }
        let is_primary = state.data.profil.profile_type.as_deref() == Some("primary");
        !is_primary
    })
    .unwrap_or(false)
}

// ---- Tests ----------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    fn empty_state() -> StoreState {
        StoreState {
            data: ProfileData::default(),
            dirty: false,
            last_flush: Instant::now(),
            mode: ProfileMode::Import,
            current_stem: None,
        }
    }

    fn state_with_uid(uid: &str) -> StoreState {
        let mut state = empty_state();
        state.data.profil.user_id = Some(uid.to_string());
        state
    }

    // -- Profil fields (Option<String>) --

    #[test]
    fn empty_profil_returns_none() {
        let state = empty_state();
        assert!(state.get("ScopelyProfile.UserId").is_none());
        assert!(!state.contains("ScopelyProfile.UserId"));
    }

    #[test]
    fn put_profil_user_id() {
        let mut state = empty_state();
        assert!(state.put("ScopelyProfile.UserId", "abc123"));
        assert_eq!(state.data.profil.user_id, Some("abc123".to_string()));
        assert!(state.dirty);
        assert!(state.contains("ScopelyProfile.UserId"));
    }

    #[test]
    fn put_profil_empty_string_is_some() {
        let mut state = empty_state();
        state.put("social_username", "");
        // Some("") is different from None
        assert_eq!(state.data.profil.social_username, Some("".to_string()));
        assert!(state.contains("social_username"));
        assert_eq!(state.get("social_username"), Some(""));
    }

    #[test]
    fn put_profil_unchanged_not_dirty() {
        let mut state = empty_state();
        state.put("social_username", "Nabor");
        state.dirty = false;
        assert!(!state.put("social_username", "Nabor"));
        assert!(!state.dirty);
    }

    // -- Auth fields --

    #[test]
    fn empty_auth_returns_none() {
        let state = empty_state();
        assert!(state.get("selected_authentication").is_none());
        assert!(!state.contains("selected_authentication"));
    }

    #[test]
    fn put_auth_string() {
        let mut state = empty_state();
        assert!(state.put("selected_authentication", "DeviceTokenAuthentication"));
        assert!(state.contains("selected_authentication"));
        assert_eq!(state.get("selected_authentication"), Some("DeviceTokenAuthentication"));
    }

    #[test]
    fn auth_int_field_string_read_returns_none() {
        let mut state = empty_state();
        // allow_association is an Int field, reading as String should return None
        // (so the hook falls through to Registry and gets "")
        state.put_int("accounts/3/scopely_id/allow_association", 0);
        assert!(state.get("accounts/3/scopely_id/allow_association").is_none());
        assert_eq!(state.get_int("accounts/3/scopely_id/allow_association"), Some(0));
        // But contains should still return true (it has a value)
        assert!(state.contains("accounts/3/scopely_id/allow_association"));
    }

    // -- Misc --

    #[test]
    fn put_misc_string() {
        let mut state = empty_state();
        assert!(state.put("some_misc_key", "data..."));
        assert_eq!(state.data.misc["some_misc_key"], toml::Value::String("data...".into()));
        assert!(state.dirty);
    }

    #[test]
    fn put_misc_int() {
        let mut state = empty_state();
        assert!(state.put_int("enable_onscreen_debug", 0));
        assert_eq!(state.data.misc["enable_onscreen_debug"], toml::Value::Integer(0));
    }

    #[test]
    fn get_misc_int() {
        let mut state = empty_state();
        state.put_int("enable_onscreen_debug", 1);
        assert_eq!(state.get_int("enable_onscreen_debug"), Some(1));
    }

    #[test]
    fn unknown_key_goes_to_misc() {
        let mut state = empty_state();
        assert!(state.put("unknown_key", "value"));
        assert!(state.contains("unknown_key"));
    }

    #[test]
    fn unknown_key_not_contained_until_stored() {
        let mut state = empty_state();
        assert!(!state.contains("some_random_key"));
        state.put("some_random_key", "value");
        assert!(state.contains("some_random_key"));
    }

    // -- Factions (user-ID-prefixed keys) --

    #[test]
    fn put_factions_key() {
        let mut state = state_with_uid("abc123");
        assert!(state.put("abc123:factions_federation_pips_hide_time", "42"));
        assert_eq!(state.data.factions["federation_pips_hide_time"], "42");
    }

    #[test]
    fn get_factions_key() {
        let mut state = state_with_uid("abc123");
        state.put("abc123:factions_temporal_pips_hide_time", "7");
        assert_eq!(state.get("abc123:factions_temporal_pips_hide_time"), Some("7"));
    }

    #[test]
    fn factions_key_wrong_uid_returns_none() {
        let state = state_with_uid("abc123");
        assert!(state.get("xyz:factions_federation_pips_hide_time").is_none());
    }

    #[test]
    fn prefixed_non_factions_goes_to_player() {
        let mut state = state_with_uid("abc123");
        assert!(state.put("abc123:chat_privatechannelslist", "data"));
        assert_eq!(
            state.data.player["chat_privatechannelslist"],
            toml::Value::String("data".into())
        );
    }

    #[test]
    fn prefixed_int_goes_to_player() {
        let mut state = state_with_uid("abc123");
        assert!(state.put_int("abc123:initial_experience_completed", 1));
        assert_eq!(
            state.data.player["initial_experience_completed"],
            toml::Value::Integer(1)
        );
    }

    #[test]
    fn no_uid_means_prefix_goes_to_misc() {
        let mut state = empty_state();
        assert!(state.put("abc123:factions_test", "value"));
        assert!(state.data.misc.contains_key("abc123:factions_test"));
    }

    // -- Filename --

    #[test]
    fn profile_stem_basic() {
        assert_eq!(profile_stem(106, "Nabor"), "106_Nabor");
    }

    #[test]
    fn profile_stem_sanitises_special_chars() {
        assert_eq!(profile_stem(42, "My Player!"), "42_My_Player_");
    }

    #[test]
    fn profile_stem_empty_name() {
        assert_eq!(profile_stem(1, ""), "1_unknown");
    }

    // -- Flush gating --

    #[test]
    fn flush_waits_for_name_and_server() {
        let mut state = empty_state();
        state.dirty = true;
        state.last_flush = Instant::now() - Duration::from_secs(60);
        assert!(state.take_pending_flush().is_none()); // no name/server

        state.data.profil.social_username = Some("Nabor".to_string());
        assert!(state.take_pending_flush().is_none()); // no server

        state.data.profil.server_instance_id = Some(106);
        state.dirty = true;
        let (content, filename) = state.take_pending_flush().unwrap();
        assert!(content.contains("Nabor"));
        assert_eq!(filename, "106_Nabor.toml");
    }

    // -- Serialisation --

    #[test]
    fn serialise_roundtrip() {
        let mut state = state_with_uid("id123");
        state.put("social_username", "TestUser");
        state.put("DownloadCacheHistoryV2", "cache data");
        state.put("id123:factions_federation_pips_hide_time", "42");
        state.put("id123:factions_klingon_pips_hide_time", "7");

        let serialised = toml::to_string_pretty(&state.data).unwrap();
        let parsed: ProfileData = toml::from_str(&serialised).unwrap();

        assert_eq!(parsed.profil.user_id, Some("id123".to_string()));
        assert_eq!(parsed.profil.social_username, Some("TestUser".to_string()));
        assert_eq!(parsed.cache["DownloadCacheHistoryV2"], toml::Value::String("cache data".into()));
        assert_eq!(parsed.factions["federation_pips_hide_time"], "42");
        assert_eq!(parsed.factions["klingon_pips_hide_time"], "7");
    }

    // -- Two-phase routing (can_route) --

    #[test]
    fn phase1_misc_not_routable() {
        let state = empty_state();
        assert!(!state.can_route("some_unknown_key"));
        assert!(!state.can_route("i9170ba:factions_test"));
        assert!(!state.can_route("i9170bachatHist"));
    }

    #[test]
    fn phase1_explicit_keys_routable() {
        let state = empty_state();
        assert!(state.can_route("ScopelyProfile.UserId"));
        assert!(state.can_route("selected_authentication"));
        assert!(state.can_route("accounts/3/adhoc_username"));
        assert!(state.can_route("chat_tabpreference"));
        assert!(state.can_route("recent_emojis"));
        assert!(state.can_route("DownloadCacheHistoryV2"));
        assert!(state.can_route("accounts/3/server_instance_id"));
    }

    #[test]
    fn phase2_everything_routable() {
        let state = state_with_uid("abc123");
        assert!(state.can_route("some_unknown_key"));
        assert!(state.can_route("abc123:factions_test"));
        assert!(state.can_route("abc123chatHist"));
    }
}
