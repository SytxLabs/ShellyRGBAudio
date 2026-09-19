use std::{path::PathBuf, time::Duration};

use serde::Serialize;
use serde_json::Value;
use tauri::{AppHandle, State, Wry, ipc::Invoke};

use crate::{
    capture::{self, AppInfo, AudioDeviceInfo},
    color::{self, ColorEngine},
    config::{self, AppConfig, LayoutSelector, LoadOutcome},
    devices::{self, Device, DeviceTypeInfo, Frame, hue::HueLightInfo},
    engine::EngineStatus,
    spatial::{SpeakerLayout, SpeakerPlacement},
};

use crate::{
    AppState, LogLine, apply_theme, config_for_engine,
    error::{AppError, AppResult, config_err, device_err, io_err},
    paths::{self, AppPrefs, Language, Theme},
};

pub fn handlers() -> impl Fn(Invoke<Wry>) -> bool + Send + Sync + 'static {
    tauri::generate_handler![
        load_config,
        save_config,
        config_path,
        set_config_path,
        engine_status,
        engine_start,
        engine_stop,
        engine_reload,
        list_audio_devices,
        list_audio_apps,
        device_types,
        resolve_speakers,
        test_device,
        hue_pair,
        hue_lights,
        recent_logs,
        get_prefs,
        set_theme,
        set_language,
    ]
}

#[derive(Serialize)]
pub struct ConfigPayload { pub path: String, pub config: AppConfig, pub warnings: Vec<String>, pub recovered: Option<Recovered>, pub missing: bool, }
#[derive(Serialize)]
pub struct Recovered { pub backup: Option<String>, pub error: String, }

#[tauri::command(async)]
fn load_config(state: State<'_, AppState>) -> AppResult<ConfigPayload> {
    let path = state.config_path();
    let outcome = config::load(&path).map_err(config_err)?;
    let path = show(&path);

    Ok(match outcome {
        LoadOutcome::Loaded { cfg, warnings } => ConfigPayload { path, config: cfg, warnings, recovered: None, missing: false },
        LoadOutcome::Missing { cfg } => ConfigPayload { path, config: cfg, warnings: Vec::new(), recovered: None, missing: true },
        LoadOutcome::Recovered { cfg, backup, error } => ConfigPayload { path, config: cfg, warnings: Vec::new(), recovered: Some(Recovered { backup: backup.as_deref().map(show), error }), missing: false, },
    })
}

#[tauri::command(async)]
fn save_config(state: State<'_, AppState>, config: Value) -> AppResult<Vec<String>> {
    let cfg: AppConfig = serde_json::from_value(config).map_err(|e| AppError::Config(format!("a setting has an unusable value or type: {e}")))?;
    let mut warnings = Vec::new();
    let bands = color::validate_bands(&cfg.bands, &mut warnings);
    let _ = ColorEngine::new(&bands, &cfg.color_map, &mut warnings);

    config::save(&state.config_path(), &cfg).map_err(io_err)?;
    Ok(warnings)
}

#[tauri::command(async)]
fn config_path(state: State<'_, AppState>) -> String {
    show(&state.config_path())
}

#[tauri::command(async)]
fn set_config_path(app: AppHandle, state: State<'_, AppState>, path: String, move_file: bool) -> AppResult<String> {
    let target = PathBuf::from(path.trim());
    if target.as_os_str().is_empty() {
        return Err(AppError::Config("the config path cannot be empty".to_string()));
    }

    let current = state.config_path();
    if move_file {
        paths::move_config(&current, &target).map_err(io_err)?;
    }

    let mut prefs = state.prefs();
    prefs.config_path = Some(target.clone());
    paths::save_prefs(&app, &prefs).map_err(io_err)?;
    state.set_prefs(prefs);
    state.set_config_path(target.clone());

    Ok(show(&target))
}

#[tauri::command(async)]
fn engine_status(state: State<'_, AppState>) -> EngineStatus {
    state.engine.status()
}

#[tauri::command(async)]
fn engine_start(state: State<'_, AppState>) -> AppResult<()> {
    state.engine.start(config_for_engine(&state.config_path())).map_err(crate::error::engine_err)
}

#[tauri::command(async)]
fn engine_stop(state: State<'_, AppState>) {
    state.engine.stop();
}

#[tauri::command(async)]
fn engine_reload(state: State<'_, AppState>) -> AppResult<()> {
    let cfg = config_for_engine(&state.config_path());
    state.engine.restart(cfg).map_err(crate::error::engine_err)
}

#[tauri::command(async)]
fn list_audio_devices() -> AppResult<Vec<AudioDeviceInfo>> {
    capture::enumerate_devices().map_err(|e| AppError::Engine(format!("{e:#}")))
}

#[tauri::command(async)]
fn list_audio_apps() -> AppResult<Vec<AppInfo>> {
    capture::enumerate_apps().map_err(|e| AppError::Engine(format!("{e:#}")))
}

#[tauri::command(async)]
fn device_types() -> Vec<DeviceTypeInfo> {
    devices::type_infos()
}

#[tauri::command(async)]
fn resolve_speakers(layout: LayoutSelector, channels: Option<usize>) -> Vec<SpeakerPlacement> {
    let mut ignored = Vec::new();
    SpeakerLayout::from_count(channels.unwrap_or(2)).resolve(&layout, &mut ignored).placements()
}

#[tauri::command(async)]
fn test_device(state: State<'_, AppState>, entry: Value) -> AppResult<String> {
    let was_running = state.engine.is_running();
    if was_running {
        state.engine.stop();
    }

    let result = flash(&entry);

    if was_running && let Err(e) = state.engine.start(config_for_engine(&state.config_path())) {
        crate::log_error!("Engine: could not resume after the device test: {e:#}");
    }
    result
}

/// Asks the bridge for an application key. Only works within half a minute of the link button being pressed, so the UI says that first.
#[tauri::command(async)]
fn hue_pair(bridge: String) -> AppResult<String> {
    devices::hue::pair(&bridge).map_err(device_err)
}

/// The lights of a bridge, so a device entry can be filled from a list rather than from resource ids typed by hand.
#[tauri::command(async)]
fn hue_lights(bridge: String, application_key: String) -> AppResult<Vec<HueLightInfo>> {
    devices::hue::list_lights(&bridge, &application_key).map_err(device_err)
}

fn flash(entry: &Value) -> AppResult<String> {
    let device: Box<dyn Device> = devices::build_one(entry).map_err(device_err)?;
    let name = device.name();

    let frame = Frame { r: 255, g: 255, b: 255, w: 0, overall: 1.0, transition_ms: 200 };
    device.apply(&frame).map_err(device_err)?;
    std::thread::sleep(Duration::from_millis(900));
    device.restore().map_err(device_err)?;

    Ok(name)
}

#[tauri::command(async)]
fn recent_logs(state: State<'_, AppState>) -> Vec<LogLine> {
    state.recent_logs()
}

#[tauri::command(async)]
fn get_prefs(state: State<'_, AppState>) -> AppPrefs {
    state.prefs()
}

#[tauri::command(async)]
fn set_theme(app: AppHandle, state: State<'_, AppState>, theme: Theme) -> AppResult<()> {
    let mut prefs = state.prefs();
    prefs.theme = theme;
    paths::save_prefs(&app, &prefs).map_err(io_err)?;
    state.set_prefs(prefs);
    apply_theme(&app, theme);
    Ok(())
}

#[tauri::command(async)]
fn set_language(app: AppHandle, state: State<'_, AppState>, setting: Language, effective: Language) -> AppResult<()> {
    let mut prefs = state.prefs();
    prefs.language = setting;
    paths::save_prefs(&app, &prefs).map_err(io_err)?;
    state.set_prefs(prefs);
    state.set_effective_language(crate::i18n::effective(effective));
    crate::tray::relabel(&app);
    Ok(())
}

fn show(path: &std::path::Path) -> String {
    path.display().to_string()
}
