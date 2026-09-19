pub mod analysis;
pub mod audio;
pub mod capture;
pub mod color;
pub mod config;
pub mod devices;
pub mod engine;
pub mod log;
pub mod spatial;

mod commands;
mod error;
mod i18n;
mod paths;
mod tray;

use std::{
    collections::VecDeque,
    path::{Path, PathBuf},
    sync::{Arc, Mutex, OnceLock, RwLock},
};

use serde::Serialize;
use tauri::{AppHandle, Emitter, Manager, RunEvent, WindowEvent};

use crate::{
    config::{AppConfig, LoadOutcome},
    engine::{Engine, EngineStatus, StatusSink},
    log::{LogLevel, LogSink},
    paths::{AppPrefs, Language, Theme},
};

const LOG_HISTORY: usize = 500;

pub const EVENT_STATUS: &str = "engine://status";
pub const EVENT_LOG: &str = "engine://log";
pub const MAIN_WINDOW: &str = "main";

#[derive(Debug, Clone, Serialize)]
pub struct LogLine {
    pub level: LogLevel,
    pub message: String,
}

pub struct AppState {
    pub engine: Engine,
    config_path: RwLock<PathBuf>,
    prefs: RwLock<AppPrefs>,
    effective_language: RwLock<Language>,
    logs: Mutex<VecDeque<LogLine>>,
    pub tray: OnceLock<tray::TrayHandles>,
}

impl AppState {
    pub fn config_path(&self) -> PathBuf { self.config_path.read().unwrap_or_else(|e| e.into_inner()).clone() }
    pub fn set_config_path(&self, path: PathBuf) { *self.config_path.write().unwrap_or_else(|e| e.into_inner()) = path; }
    pub fn prefs(&self) -> AppPrefs {
        self.prefs.read().unwrap_or_else(|e| e.into_inner()).clone()
    }
    pub fn set_prefs(&self, prefs: AppPrefs) { *self.prefs.write().unwrap_or_else(|e| e.into_inner()) = prefs; }
    pub fn effective_language(&self) -> Language { *self.effective_language.read().unwrap_or_else(|e| e.into_inner()) }
    pub fn set_effective_language(&self, language: Language) { *self.effective_language.write().unwrap_or_else(|e| e.into_inner()) = language; }
    pub fn recent_logs(&self) -> Vec<LogLine> { self.logs.lock().unwrap_or_else(|e| e.into_inner()).iter().cloned().collect() }
    fn push_log(&self, line: LogLine) {
        let mut logs = self.logs.lock().unwrap_or_else(|e| e.into_inner());
        if logs.len() == LOG_HISTORY {
            logs.pop_front();
        }
        logs.push_back(line);
    }
}

pub fn config_for_engine(path: &Path) -> AppConfig {
    match config::load(path) {
        Ok(LoadOutcome::Loaded { cfg, .. }) => cfg,
        Ok(LoadOutcome::Missing { cfg }) => {
            log::emit(LogLevel::Warn, format_args!("Config: {} does not exist yet, running with defaults.", path.display()));
            cfg
        }
        Ok(LoadOutcome::Recovered { cfg, backup, error }) => {
            match backup {
                Some(b) => log::emit(LogLevel::Error, format_args!("Config: {error}. A copy is at {}; running with defaults.", b.display())),
                None => log::emit(LogLevel::Error, format_args!("Config: {error}. Running with defaults.")),
            }
            cfg
        }
        Err(e) => {
            log::emit(LogLevel::Error, format_args!("Config: {} could not be read ({e:#}). Running with defaults.", path.display()));
            AppConfig::default()
        }
    }
}

pub fn show_settings(app: &AppHandle) {
    let Some(window) = app.get_webview_window(MAIN_WINDOW) else { return };
    let _ = window.show();
    let _ = window.unminimize();
    let _ = window.set_focus();
}

pub fn apply_theme(app: &AppHandle, theme: Theme) {
    let Some(window) = app.get_webview_window(MAIN_WINDOW) else { return };
    let _ = window.set_theme(match theme {
        Theme::System => None,
        Theme::Light => Some(tauri::Theme::Light),
        Theme::Dark => Some(tauri::Theme::Dark),
    });
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    let builder = tauri::Builder::default();

    #[cfg(desktop)]
    let builder = builder.plugin(tauri_plugin_single_instance::init(|app, _args, _cwd| show_settings(app)));

    let app = builder.plugin(tauri_plugin_dialog::init()).plugin(tauri_plugin_opener::init()).invoke_handler(commands::handlers()).setup(|app| {
        let handle = app.handle().clone();

        let prefs = paths::load_prefs(&handle);
        let config_path = paths::resolve_config_path(&handle, &prefs);
        let theme = prefs.theme;

        let status_sink: StatusSink = {
            let handle = handle.clone();
            Arc::new(move |status: &EngineStatus| {
                let _ = handle.emit(EVENT_STATUS, status);
                tray::refresh(&handle, status);
            })
        };

        app.manage(AppState { engine: Engine::new(status_sink), config_path: RwLock::new(config_path.clone()), effective_language: RwLock::new(i18n::effective(prefs.language)), prefs: RwLock::new(prefs), logs: Mutex::new(VecDeque::with_capacity(LOG_HISTORY)), tray: OnceLock::new(), });

        let log_sink: LogSink = {
            let handle = handle.clone();
            Arc::new(move |level: LogLevel, message: &str| {
                if cfg!(debug_assertions) { eprintln!("{message}"); }
                let line = LogLine { level, message: message.to_string() };
                handle.state::<AppState>().push_log(line.clone());
                let _ = handle.emit(EVENT_LOG, line);
            })
        };
        log::set_sink(log_sink);

        tray::build(app)?;
        apply_theme(&handle, theme);

        let state = handle.state::<AppState>();
        if let Err(e) = state.engine.start(config_for_engine(&config_path)) { log::emit(LogLevel::Error, format_args!("Engine: could not start: {e:#}")); }
        Ok(())
    }).on_window_event(|window, event| {
        if let WindowEvent::CloseRequested { api, .. } = event {
            api.prevent_close();
            let _ = window.hide();
        }
    }).build(tauri::generate_context!()).expect("error while building the application");

    app.run(|handle, event| {
        if let RunEvent::Exit = event {
            handle.state::<AppState>().engine.stop();
            log::clear_sink();
        }
    });
}
