use tauri::{App, AppHandle, Manager, Wry, menu::{Menu, MenuEvent, MenuItem, PredefinedMenuItem}, tray::{MouseButton, MouseButtonState, TrayIconBuilder, TrayIconEvent} };
use crate::{AppState, config_for_engine, i18n, engine::{EngineState, EngineStatus}, log::{self, LogLevel}, paths::Language, show_settings };

pub struct TrayHandles { status: MenuItem<Wry>, toggle: MenuItem<Wry>, settings: MenuItem<Wry>, reload: MenuItem<Wry>, quit: MenuItem<Wry> }

pub fn build(app: &mut App) -> tauri::Result<()> {
    let language = app.handle().state::<AppState>().prefs().language;

    let title = MenuItem::with_id(app, "title", "ShellyRGBAudio", false, None::<&str>)?;
    let status = MenuItem::with_id(app, "status", i18n::starting(language), false, None::<&str>)?;
    let toggle = MenuItem::with_id(app, "toggle", i18n::pause(language), true, None::<&str>)?;
    let settings = MenuItem::with_id(app, "settings", i18n::settings(language), true, None::<&str>)?;
    let reload = MenuItem::with_id(app, "reload", i18n::reload(language), true, None::<&str>)?;
    let quit = MenuItem::with_id(app, "quit", i18n::quit(language), true, None::<&str>)?;

    let menu = Menu::with_items(app, &[&title, &status, &PredefinedMenuItem::separator(app)?, &toggle, &settings, &reload, &PredefinedMenuItem::separator(app)?, &quit])?;

    TrayIconBuilder::with_id("main")
        .icon(app.default_window_icon().cloned().expect("the bundle always carries a window icon"))
        .tooltip("ShellyRGBAudio")
        .menu(&menu)
        .show_menu_on_left_click(false)
        .on_menu_event(on_menu_event)
        .on_tray_icon_event(|tray, event| {
            if let TrayIconEvent::Click { button: MouseButton::Left, button_state: MouseButtonState::Up, .. } = event {
                show_settings(tray.app_handle());
            }
        })
        .build(app)?;

    let handle = app.handle().clone();
    let _ = handle.state::<AppState>().tray.set(TrayHandles { status, toggle, settings, reload, quit });
    let current = handle.state::<AppState>().engine.status();
    refresh(&handle, &current);
    Ok(())
}

fn on_menu_event(app: &AppHandle, event: MenuEvent) {
    match event.id.as_ref() {
        "settings" => show_settings(app),
        "toggle" => off_thread(app, |app| {
            let state = app.state::<AppState>();
            if state.engine.is_running() {
                state.engine.stop();
            } else if let Err(e) = state.engine.start(config_for_engine(&state.config_path())) {
                log::emit(LogLevel::Error, format_args!("Engine: could not start: {e:#}"));
            }
        }),
        "reload" => off_thread(app, |app| {
            let state = app.state::<AppState>();
            let cfg = config_for_engine(&state.config_path());
            if let Err(e) = state.engine.restart(cfg) {
                log::emit(LogLevel::Error, format_args!("Engine: could not restart: {e:#}"));
            }
        }),
        "quit" => app.exit(0),
        _ => {}
    }
}

fn off_thread(app: &AppHandle, f: impl FnOnce(&AppHandle) + Send + 'static) {
    let app = app.clone();
    tauri::async_runtime::spawn_blocking(move || f(&app));
}

pub fn refresh(app: &AppHandle, status: &EngineStatus) {
    let language = i18n::effective(app.state::<AppState>().effective_language());
    let line = describe(status, language);
    let toggle = match status.state {
        EngineState::Running | EngineState::Starting => i18n::pause(language),
        EngineState::Stopped | EngineState::Failed => i18n::resume(language),
    };

    let handle = app.clone();
    let _ = app.run_on_main_thread(move || {
        let state = handle.state::<AppState>();
        let Some(tray) = state.tray.get() else { return };
        let _ = tray.status.set_text(&line);
        let _ = tray.toggle.set_text(toggle);
    });
}

pub fn relabel(app: &AppHandle) {
    let language = i18n::effective(app.state::<AppState>().effective_language());
    let handle = app.clone();
    let _ = app.run_on_main_thread(move || {
        let state = handle.state::<AppState>();
        let Some(tray) = state.tray.get() else { return };
        let _ = tray.settings.set_text(i18n::settings(language));
        let _ = tray.reload.set_text(i18n::reload(language));
        let _ = tray.quit.set_text(i18n::quit(language));
    });
    let status = app.state::<AppState>().engine.status();
    refresh(app, &status);
}

fn describe(status: &EngineStatus, language: Language) -> String {
    match status.state {
        EngineState::Stopped => i18n::stopped(language).to_string(),
        EngineState::Starting => i18n::starting(language).to_string(),
        EngineState::Failed => {
            let reason = status.error.as_deref().unwrap_or(i18n::unknown_error(language));
            i18n::error_prefix(language, &first_line(reason, 60))
        }
        EngineState::Running => {
            let mut parts = vec![i18n::running(language).to_string()];
            if let Some(hz) = status.sample_rate {
                parts.push(khz(hz));
            }
            if let Some(layout) = &status.layout {
                parts.push(layout.clone());
            }
            parts.push(i18n::device_count(language, status.devices.len()));
            parts.join(" · ")
        }
    }
}

fn khz(hz: f32) -> String {
    let khz = hz / 1000.0;
    match (khz - khz.round()).abs() < 0.05 { true => format!("{khz:.0} kHz"), false => format!("{khz:.1} kHz") }
}

fn first_line(text: &str, max: usize) -> String {
    let line = text.lines().next().unwrap_or(text);
    match line.char_indices().nth(max) { Some((cut, _)) => format!("{}…", &line[..cut]), None => line.to_string() }
}
