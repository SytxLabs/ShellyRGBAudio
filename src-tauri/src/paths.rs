use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};
use std::{env, fs, path::{Path, PathBuf}};
use tauri::{AppHandle, Manager};

pub const PREFS_FILE: &str = "app.json";
pub const CONFIG_FILE: &str = "config.json";

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Theme {
    #[default]
    System,
    Light,
    Dark,
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
pub enum Language {
    #[default]
    #[serde(rename = "system")]
    System,
    #[serde(rename = "en")]
    English,
    #[serde(rename = "de", alias = "de")]
    German,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct AppPrefs {
    pub config_path: Option<PathBuf>,
    pub theme: Theme,
    pub language: Language,
}

pub fn prefs_path(app: &AppHandle) -> Result<PathBuf> {
    Ok(app.path().app_config_dir().context("no per-user config directory")?.join(PREFS_FILE))
}

pub fn load_prefs(app: &AppHandle) -> AppPrefs {
    let Ok(path) = prefs_path(app) else { return AppPrefs::default() };
    let Ok(raw) = fs::read_to_string(&path) else { return AppPrefs::default() };
    match serde_json::from_str(raw.trim_start_matches('\u{feff}')) {
        Ok(prefs) => prefs,
        Err(e) => {
            crate::log_warn!("Could not read {}: {e}. Using defaults.", path.display());
            AppPrefs::default()
        }
    }
}

pub fn save_prefs(app: &AppHandle, prefs: &AppPrefs) -> Result<()> {
    let path = prefs_path(app)?;
    if let Some(dir) = path.parent() {
        fs::create_dir_all(dir).with_context(|| format!("create {}", dir.display()))?;
    }
    let text = serde_json::to_string_pretty(prefs).context("serialize the app preferences")?;
    fs::write(&path, text).with_context(|| format!("write {}", path.display()))?;
    Ok(())
}

pub fn resolve_config_path(app: &AppHandle, prefs: &AppPrefs) -> PathBuf {
    if let Some(path) = from_cli() {
        return path;
    }
    if let Some(value) = env::var_os("SHELLYRGBAUDIO_CONFIG").filter(|v| !v.is_empty()) {
        return PathBuf::from(value);
    }
    if let Some(path) = prefs.config_path.clone().filter(|p| !p.as_os_str().is_empty()) {
        return path;
    }
    if let Some(path) = portable() {
        return path;
    }
    default_config_path(app)
}

fn from_cli() -> Option<PathBuf> {
    let mut args = env::args_os().skip(1);
    while let Some(arg) = args.next() {
        let text = arg.to_string_lossy().into_owned();
        if let Some(value) = text.strip_prefix("--config=") {
            return Some(PathBuf::from(value));
        }
        if text == "--config" {
            return args.next().map(PathBuf::from);
        }
    }
    None
}

fn portable() -> Option<PathBuf> {
    let path = env::current_exe().ok()?.parent()?.join(CONFIG_FILE);
    path.exists().then_some(path)
}

pub fn default_config_path(app: &AppHandle) -> PathBuf {
    match app.path().app_config_dir() {
        Ok(dir) => dir.join(CONFIG_FILE),
        Err(_) => PathBuf::from(CONFIG_FILE),
    }
}

pub fn move_config(from: &Path, to: &Path) -> Result<()> {
    if from == to || !from.exists() {
        return Ok(());
    }
    if let Some(dir) = to.parent().filter(|d| !d.as_os_str().is_empty()) {
        fs::create_dir_all(dir).with_context(|| format!("create {}", dir.display()))?;
    }
    match fs::rename(from, to) {
        Ok(()) => Ok(()),
        Err(_) => {
            fs::copy(from, to).with_context(|| format!("copy {} to {}", from.display(), to.display()))?;
            fs::remove_file(from).with_context(|| format!("remove {}", from.display()))
        }
    }
}
