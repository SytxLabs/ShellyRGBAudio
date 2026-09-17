mod analysis;
mod audio;
mod capture;
mod color;
mod config;
mod devices;

use crate::analysis::{Analysis, Analyzer};
use crate::capture::Capture;
use crate::color::{ColorEngine, Rgbw};
use crate::devices::{BrightnessLimits, Frame, RuntimeDevice};
use anyhow::Result;
use std::{
    env,
    sync::{
        atomic::{AtomicBool, Ordering},
        mpsc::{self, RecvTimeoutError},
        Arc,
    },
    thread,
    time::{Duration, Instant},
};

enum Msg {
    Sound(Analysis),
    Silence,
}

fn main() -> Result<()> {
    let Some(path) = parse_args()? else { return Ok(()) };
    let cfg = config::load_or_create(&path)?;

    let mut warnings = Vec::new();
    let bands = color::validate_bands(&cfg.bands, &mut warnings);
    let global_engine = Arc::new(ColorEngine::new(&bands, &cfg.color_map, &mut warnings));

    devices::set_brightness_limits(BrightnessLimits {
        floor: cfg.output.brightness_floor,
        gamma_min: cfg.output.gamma_min,
        gamma_max: cfg.output.gamma_max,
    });

    let runtime: Arc<Vec<RuntimeDevice>> = Arc::new(devices::build_all(&cfg.devices, &bands, &cfg.color_map, &mut warnings)?);
    for w in &warnings {
        eprintln!("Config: {w}");
    }
    if runtime.is_empty() {
        eprintln!("Warn: no devices configured, nothing will be controlled.");
    }
    for dev in runtime.iter() {
        let engine = dev.engine.as_deref().unwrap_or(&global_engine);
        eprintln!("Device: {}{}", dev.name(), if dev.engine.is_some() { " (own color map)" } else { "" });
        eprintln!("  {}", describe_bands(&bands, engine));
    }
    if runtime.is_empty() {
        eprintln!("Color map: {}", describe_bands(&bands, &global_engine));
    }

    let running = Arc::new(AtomicBool::new(true));
    {
        let running = Arc::clone(&running);
        ctrlc::set_handler(move || { running.store(false, Ordering::SeqCst); })?;
    }

    let old_hook = std::panic::take_hook();
    {
        let runtime = Arc::clone(&runtime);
        std::panic::set_hook(Box::new(move |info| {
            restore_last_state(&runtime);
            old_hook(info);
        }));
    }

    let (tx, rx) = mpsc::channel::<Msg>();
    let _sender_thread = spawn_sender(Arc::clone(&runtime), Arc::clone(&global_engine), &cfg, bands.len(), rx);

    let mut warnings = Vec::new();
    let capture = Capture::start(&cfg.audio, Arc::clone(&running), &mut warnings)?;
    let mut analyzer = Analyzer::new(&cfg.audio, &bands, &cfg.dynamics, &cfg.output, capture.sample_rate(), &mut warnings);
    for w in &warnings {
        eprintln!("Audio: {w}");
    }
    eprintln!("Audio: capturing {} at {:.0} Hz", capture.source(), capture.sample_rate());

    let silence_timeout = Duration::from_millis(cfg.audio.silence_timeout_ms.max(1));
    let mut dimmed_due_to_silence = false;

    while running.load(Ordering::SeqCst) {
        match capture.recv_timeout(silence_timeout) {
            Ok(chunk) => {
                for sample in chunk {
                    let Some(result) = analyzer.push(sample) else { continue };
                    if result.has_signal {
                        dimmed_due_to_silence = false;
                    }
                    let _ = tx.send(Msg::Sound(result));
                }
            }
            Err(RecvTimeoutError::Timeout) => {
                if !dimmed_due_to_silence {
                    let _ = tx.send(Msg::Silence);
                    dimmed_due_to_silence = true;
                }
            }
            Err(RecvTimeoutError::Disconnected) => break, // The capture threads are gone, nothing left to react to.
        }
    }

    restore_last_state(&runtime);
    Ok(())
}

fn parse_args() -> Result<Option<String>> {
    let mut path: Option<String> = None;
    let mut devices = false;
    let mut apps = false;

    for arg in env::args().skip(1) {
        match arg.as_str() {
            "--list-devices" => devices = true,
            "--list-apps" => apps = true,
            "-h" | "--help" => {
                usage();
                return Ok(None);
            }
            unknown if unknown.starts_with('-') => {
                eprintln!("Unknown option {unknown:?}.");
                usage();
                return Ok(None);
            }
            file => path = Some(file.to_string()),
        }
    }

    if devices {
        capture::list_devices();
    }
    if apps {
        capture::print_apps()?;
    }
    if devices || apps {
        return Ok(None);
    }

    Ok(Some(path.or_else(|| env::var("SHELLYRGBAUDIO_CONFIG").ok()).unwrap_or_else(|| "config.json".to_string())))
}

fn usage() {
    eprintln!("Usage: ShellyRGBAudio [config.json] [--list-devices] [--list-apps]");
    eprintln!("  --list-devices   print the output devices usable in audio.device");
    eprintln!("  --list-apps      print the applications usable in audio.apps.groups[].apps");
}

/// Throttles, deduplicates and fans out to the lights. Each device keeps its own last frame, because two devices with different color maps legitimately differ on the same analysis.
fn spawn_sender(runtime: Arc<Vec<RuntimeDevice>>, global: Arc<ColorEngine>, cfg: &config::AppConfig, band_count: usize, rx: mpsc::Receiver<Msg>) -> thread::JoinHandle<()> {
    let min_send_interval = Duration::from_millis(cfg.output.change_interval_ms);
    let deadband_rgb = cfg.output.deadband_rgb as i16;
    let deadband_overall = cfg.output.deadband_overall;
    let strobe_color = cfg.dynamics.strobe_color;
    let silence_brightness = cfg.audio.silence_brightness.clamp(0.0, 1.0);
    let silence_fade_ms = cfg.audio.silence_fade_ms;

    thread::spawn(move || {
        let quiet = vec![0.0f32; band_count];
        let initial: Vec<Frame> = runtime.iter().map(|dev| {
            to_frame(dev.engine.as_deref().unwrap_or(&global).resolve(&quiet, silence_brightness), silence_brightness, silence_fade_ms)
        }).collect();

        let mut last: Vec<Option<Frame>> = vec![None; runtime.len()];
        let mut last_sent = Instant::now() - min_send_interval;

        while let Ok(mut msg) = rx.recv() {
            while let Ok(newer) = rx.try_recv() {
                msg = newer;
            }
            if last_sent.elapsed() < min_send_interval {
                continue;
            }

            let mut sent_any = false;
            for (i, dev) in runtime.iter().enumerate() {
                let (frame, force) = match &msg {
                    Msg::Sound(a) => {
                        let mut color = dev.engine.as_deref().unwrap_or(&global).resolve(&a.bands, a.overall);
                        if a.strobing && let Some(c) = strobe_color {
                            color = c;
                        }
                        (to_frame(color, a.overall, a.transition_ms), a.force)
                    }
                    Msg::Silence => {
                        let base = last[i].unwrap_or(initial[i]);
                        (Frame { overall: silence_brightness, transition_ms: silence_fade_ms, ..base }, true)
                    }
                };

                if !force && !changed(&frame, last[i].as_ref(), deadband_rgb, deadband_overall) {
                    continue;
                }
                if let Err(e) = dev.apply(&frame) {
                    eprintln!("{}: apply failed (ignored): {e:#}", dev.name());
                }
                last[i] = Some(frame);
                sent_any = true;
            }

            if sent_any {
                last_sent = Instant::now();
            }
        }
    })
}

fn describe_bands(bands: &[color::BandConfig], engine: &ColorEngine) -> String {
    bands.iter().zip(engine.band_colors()).zip(engine.band_weights())
        .map(|((b, c), w)| format!("{} {:.0}-{:.0}Hz {}{}", b.name, b.from_hz, b.to_hz, c.to_hex(), if *w == 1.0 { String::new() } else { format!(" x{w}") }))
        .collect::<Vec<_>>().join(", ")
}
fn to_frame(c: Rgbw, overall: f32, transition_ms: u32) -> Frame {
    Frame { r: c.r, g: c.g, b: c.b, w: c.w, overall, transition_ms }
}
fn changed(next: &Frame, last: Option<&Frame>, deadband_rgb: i16, deadband_overall: f32) -> bool {
    let Some(prev) = last else { return true };
    let d = |a: u8, b: u8| (a as i16 - b as i16).abs() > deadband_rgb;
    d(next.r, prev.r) || d(next.g, prev.g) || d(next.b, prev.b) || d(next.w, prev.w) || (next.overall - prev.overall).abs() > deadband_overall
}
fn restore_last_state(runtime: &[RuntimeDevice]) {
    for dev in runtime.iter() {
        if let Err(e) = dev.restore() {
            eprintln!("{}: restore failed (ignored): {e:#}", dev.name());
        }
    }
}
