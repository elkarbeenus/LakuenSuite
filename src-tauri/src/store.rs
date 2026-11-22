//! Einstellungen und Installationsstand, lokal als JSON gespeichert.

use serde::{de::DeserializeOwned, Deserialize, Serialize};
use std::collections::HashMap;
use std::path::{Path, PathBuf};

pub const OWNER: &str = "elkarbeenus";
pub const SELF_REPO: &str = "LakuenSuite";

fn home() -> PathBuf {
    dirs::home_dir().unwrap_or_else(|| PathBuf::from("/tmp"))
}

pub fn config_dir() -> PathBuf {
    dirs::config_dir().unwrap_or_else(|| home().join(".config")).join("lakuensuite")
}
pub fn data_dir() -> PathBuf {
    dirs::data_dir().unwrap_or_else(|| home().join(".local/share")).join("lakuensuite")
}
pub fn cache_dir() -> PathBuf {
    dirs::cache_dir().unwrap_or_else(|| home().join(".cache")).join("lakuensuite")
}
pub fn icons_dir() -> PathBuf {
    data_dir().join("icons")
}
pub fn applications_dir() -> PathBuf {
    dirs::data_dir().unwrap_or_else(|| home().join(".local/share")).join("applications")
}
pub fn default_install_dir() -> String {
    home().join("Applications").join("LakuenSuite").to_string_lossy().to_string()
}

#[derive(Serialize, Deserialize, Clone, Debug)]
#[serde(default)]
pub struct Settings {
    pub install_dir: String,
    pub theme: String,
    pub check_on_start: bool,
    pub auto_update_apps: bool,
    pub auto_update_self: bool,
    pub desktop_entries: bool,
    pub keep_previous: bool,
    pub show_forks: bool,
    pub show_archived: bool,
    pub github_token: String,
    pub view: String,
    pub sort: String,
    pub favorites: Vec<String>,
    pub hidden: Vec<String>,
    pub tutorial_done: bool,
    pub refresh_minutes: u32,
}

impl Default for Settings {
    fn default() -> Self {
        Settings {
            install_dir: default_install_dir(),
            theme: "nacht".into(),
            check_on_start: true,
            auto_update_apps: true,
            auto_update_self: false,
            desktop_entries: true,
            keep_previous: true,
            show_forks: true,
            show_archived: false,
            github_token: String::new(),
            view: "grid".into(),
            sort: "status".into(),
            favorites: vec![],
            hidden: vec![],
            tutorial_done: false,
            refresh_minutes: 30,
        }
    }
}

#[derive(Serialize, Deserialize, Clone, Debug, Default)]
#[serde(default)]
pub struct Installed {
    pub tag: String,
    pub asset_id: u64,
    pub asset_name: String,
    pub path: String,
    pub sha256: String,
    pub verified: bool,
    pub installed_at: u64,
    pub size: u64,
    pub prev_tag: Option<String>,
    pub prev_asset_id: Option<u64>,
    /// Nach einem Zurücksetzen: diese Version nicht automatisch wieder installieren.
    pub hold_tag: Option<String>,
    pub icon: Option<String>,
    pub launches: u64,
    pub last_launch: Option<u64>,
}

#[derive(Serialize, Deserialize, Clone, Debug, Default)]
#[serde(default)]
pub struct State {
    pub installed: HashMap<String, Installed>,
}

pub fn load<T: DeserializeOwned + Default>(path: &Path) -> T {
    std::fs::read_to_string(path)
        .ok()
        .and_then(|s| serde_json::from_str(&s).ok())
        .unwrap_or_default()
}

pub fn save<T: Serialize>(path: &Path, value: &T, private: bool) -> Result<(), String> {
    if let Some(dir) = path.parent() {
        std::fs::create_dir_all(dir).map_err(|e| e.to_string())?;
    }
    let tmp = path.with_extension("json.tmp");
    let data = serde_json::to_string_pretty(value).map_err(|e| e.to_string())?;
    std::fs::write(&tmp, data).map_err(|e| e.to_string())?;
    #[cfg(unix)]
    if private {
        use std::os::unix::fs::PermissionsExt;
        let _ = std::fs::set_permissions(&tmp, std::fs::Permissions::from_mode(0o600));
    }
    #[cfg(not(unix))]
    let _ = private;
    std::fs::rename(&tmp, path).map_err(|e| e.to_string())
}

pub fn settings_file() -> PathBuf {
    config_dir().join("settings.json")
}
pub fn state_file() -> PathBuf {
    data_dir().join("state.json")
}
pub fn http_cache_file() -> PathBuf {
    cache_dir().join("http-cache.json")
}

pub fn now() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0)
}
