use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};
use std::{
    sync::{
        Arc, Mutex, MutexGuard, Once, OnceLock, RwLock, Weak,
        atomic::{AtomicBool, Ordering},
        mpsc::{self, RecvTimeoutError},
    },
    thread,
    time::{Duration, Instant},
};

use crate::analysis::{Analysis, Analyzer};
use crate::capture::Capture;
use crate::color::{self, BandConfig, ColorEngine, Rgbw};
use crate::config::AppConfig;
use crate::devices::{self, BrightnessLimits, Frame, RuntimeDevice};
use crate::spatial::SpeakerPlacement;
use crate::{log_error, log_info, log_warn};

const SHUTDOWN_POLL: Duration = Duration::from_millis(100);

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
#[cfg_attr(feature = "ts", derive(ts_rs::TS), ts(export))]
pub enum EngineState { Stopped, Starting, Running, Failed }

#[derive(Debug, Clone, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS), ts(export))]
pub struct EngineStatus {
    pub state: EngineState,
    pub source: Option<String>,
    pub sample_rate: Option<f32>,
    pub channels: Option<usize>,
    pub layout: Option<String>,
    pub speakers: Vec<SpeakerPlacement>,
    pub devices: Vec<String>,
    pub warnings: Vec<String>,
    pub error: Option<String>,
}

impl Default for EngineStatus {
    fn default() -> Self { Self { state: EngineState::Stopped, source: None, sample_rate: None, channels: None, layout: None, speakers: Vec::new(), devices: Vec::new(), warnings: Vec::new(), error: None } }
}

pub type StatusSink = Arc<dyn Fn(&EngineStatus) + Send + Sync>;
pub struct Engine { inner: Mutex<Option<Running>>, status: Arc<RwLock<EngineStatus>>, on_status: StatusSink }
struct Running { running: Arc<AtomicBool>, supervisor: thread::JoinHandle<()> }

impl Engine {
    pub fn new(on_status: StatusSink) -> Self {
        install_panic_hook();
        Self { inner: Mutex::new(None), status: Arc::new(RwLock::new(EngineStatus::default())), on_status }
    }
    pub fn silent() -> Self {
        Self::new(Arc::new(|_| {}))
    }
    pub fn status(&self) -> EngineStatus { self.status.read().unwrap_or_else(|e| e.into_inner()).clone() }
    pub fn is_running(&self) -> bool { lock(&self.inner).as_ref().is_some_and(|r| !r.supervisor.is_finished()) }

    pub fn start(&self, cfg: AppConfig) -> Result<()> {
        let mut guard = lock(&self.inner);
        if guard.as_ref().is_some_and(|r| r.supervisor.is_finished()) && let Some(done) = guard.take() {
            let _ = done.supervisor.join();
        }
        if guard.is_some() {
            return Ok(());
        }
        let publisher = Publisher { status: Arc::clone(&self.status), sink: Arc::clone(&self.on_status) };
        publisher.update(|s| {
            *s = EngineStatus { state: EngineState::Starting, ..EngineStatus::default() };
        });

        let running = Arc::new(AtomicBool::new(true));
        let supervisor = thread::Builder::new().name("engine".to_string()).spawn({
            let running = Arc::clone(&running);
            move || run(cfg, running, publisher)
        }).context("spawn the engine thread")?;
        *guard = Some(Running { running, supervisor });
        Ok(())
    }

    pub fn stop(&self) {
        let run = lock(&self.inner).take();
        let Some(run) = run else { return };
        run.running.store(false, Ordering::SeqCst);
        let _ = run.supervisor.join();
    }

    pub fn restart(&self, cfg: AppConfig) -> Result<()> {
        self.stop();
        self.start(cfg)
    }
}

impl Drop for Engine {
    fn drop(&mut self) {
        self.stop();
    }
}

#[derive(Clone)]
struct Publisher { status: Arc<RwLock<EngineStatus>>, sink: StatusSink }

impl Publisher {
    fn update(&self, f: impl FnOnce(&mut EngineStatus)) {
        let snapshot = {
            let mut guard = self.status.write().unwrap_or_else(|e| e.into_inner());
            f(&mut guard);
            guard.clone()
        };
        (self.sink)(&snapshot);
    }
}

fn run(cfg: AppConfig, running: Arc<AtomicBool>, publisher: Publisher) {
    match run_pipeline(&cfg, &running, &publisher) {
        Ok(()) => publisher.update(|s| *s = EngineStatus::default()),
        Err(e) => {
            let message = format!("{e:#}");
            log_error!("Engine: {message}");
            publisher.update(|s| { *s = EngineStatus { state: EngineState::Failed, error: Some(message), ..EngineStatus::default() }; });
        }
    }
}

fn run_pipeline(cfg: &AppConfig, running: &Arc<AtomicBool>, publisher: &Publisher) -> Result<()> {
    let mut warnings = Vec::new();
    let bands = color::validate_bands(&cfg.bands, &mut warnings);
    let global_engine = Arc::new(ColorEngine::new(&bands, &cfg.color_map, &mut warnings));

    devices::set_brightness_limits(BrightnessLimits {
        floor: cfg.output.brightness_floor,
        gamma_min: cfg.output.gamma_min,
        gamma_max: cfg.output.gamma_max,
    });
    let mut audio_warnings = Vec::new();
    let capture = Capture::start(&cfg.audio, Arc::clone(running), &mut audio_warnings)?;
    let layout = capture.layout().clone().resolve(&cfg.spatial.layout, &mut warnings);

    let runtime: Arc<Vec<RuntimeDevice>> = Arc::new(devices::build_all(&cfg.devices, &bands, &cfg.color_map, &cfg.spatial, &layout, &mut warnings)?);
    remember_devices(&runtime);

    for w in &warnings {
        log_warn!("Config: {w}");
    }
    for w in &audio_warnings {
        log_warn!("Audio: {w}");
    }
    log_info!("Audio: capturing {} at {:.0} Hz, {}", capture.source(), capture.sample_rate(), layout.describe());

    if runtime.is_empty() {
        log_warn!("Warn: no devices configured, nothing will be controlled.");
    }
    for dev in runtime.iter() {
        let engine = dev.engine.as_deref().unwrap_or(&global_engine);
        log_info!("Device: {}{}", dev.name(), if dev.engine.is_some() { " (own color map)" } else { "" });
        log_info!("  {}", describe_bands(&bands, engine));
        if let Some(s) = &dev.spatial {
            log_info!("  {}", s.describe(&layout));
        }
    }
    if runtime.is_empty() {
        log_info!("Color map: {}", describe_bands(&bands, &global_engine));
    }

    let (tx, rx) = mpsc::channel::<Msg>();
    let sender = spawn_sender(Arc::clone(&runtime), Arc::clone(&global_engine), cfg, bands.len(), rx)?;

    let channels = capture.channels().max(1);
    let spatial_channels = if runtime.iter().any(|d| d.spatial.is_some()) { channels } else { 0 };
    let mut analysis_warnings = Vec::new();
    let mut analyzer = Analyzer::new(&cfg.audio, &bands, &cfg.dynamics, &cfg.output, capture.sample_rate(), spatial_channels, &mut analysis_warnings);
    for w in &analysis_warnings {
        log_warn!("Audio: {w}");
    }

    publisher.update(|s| {
        s.state = EngineState::Running;
        s.source = Some(capture.source().to_string());
        s.sample_rate = Some(capture.sample_rate());
        s.channels = Some(channels);
        s.layout = Some(layout.describe());
        s.speakers = layout.placements();
        s.devices = runtime.iter().map(|d| d.name()).collect();
        s.warnings = warnings.iter().chain(&audio_warnings).chain(&analysis_warnings).cloned().collect();
        s.error = None;
    });

    let silence_timeout = Duration::from_millis(cfg.audio.silence_timeout_ms.max(1));
    let poll = silence_timeout.min(SHUTDOWN_POLL);
    let mut last_audio = Instant::now();
    let mut dimmed_due_to_silence = false;

    while running.load(Ordering::SeqCst) {
        match capture.recv_timeout(poll) {
            Ok(chunk) => {
                last_audio = Instant::now();
                for frame in chunk.chunks_exact(channels) {
                    let Some(result) = analyzer.push_frame(frame) else { continue };
                    if result.has_signal {
                        dimmed_due_to_silence = false;
                    }
                    let _ = tx.send(Msg::Sound(result));
                }
            }
            Err(RecvTimeoutError::Timeout) => {
                if !dimmed_due_to_silence && last_audio.elapsed() >= silence_timeout {
                    let _ = tx.send(Msg::Silence);
                    dimmed_due_to_silence = true;
                }
            }
            Err(RecvTimeoutError::Disconnected) => break,
        }
    }
    running.store(false, Ordering::SeqCst);
    drop(tx);
    let _ = sender.join();
    capture.stop();
    restore_last_state(&runtime);
    forget_devices();
    Ok(())
}

enum Msg { Sound(Analysis), Silence, }

fn spawn_sender(runtime: Arc<Vec<RuntimeDevice>>, global: Arc<ColorEngine>, cfg: &AppConfig, band_count: usize, rx: mpsc::Receiver<Msg>) -> Result<thread::JoinHandle<()>> {
    let min_send_interval = Duration::from_millis(cfg.output.change_interval_ms);
    let deadband_rgb = cfg.output.deadband_rgb as i16;
    let deadband_overall = cfg.output.deadband_overall;
    let strobe_color = cfg.dynamics.strobe_color;
    let silence_brightness = cfg.audio.silence_brightness.clamp(0.0, 1.0);
    let silence_fade_ms = cfg.audio.silence_fade_ms;

    thread::Builder::new().name("engine-output".to_string()).spawn(move || {
        let quiet = vec![0.0f32; band_count];
        let initial: Vec<Frame> = runtime.iter().map(|dev| {
            to_frame(dev.engine.as_deref().unwrap_or(&global).resolve(&quiet, silence_brightness), silence_brightness, silence_fade_ms)
        }).collect();

        let mut last: Vec<Option<Frame>> = vec![None; runtime.len()];
        let mut last_sent = Instant::now() - min_send_interval;
        let mut scratch: Vec<Vec<f32>> = vec![Vec::with_capacity(band_count); runtime.len()];
        let mut gradients: Vec<Vec<Rgbw>> = runtime.iter().zip(&initial).map(|(d, f)| vec![Rgbw { r: f.r, g: f.g, b: f.b, w: f.w }; if d.segment_count() > 1 { d.segment_count() } else { 0 }]).collect();
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
                        let bands = match &dev.spatial {
                            Some(s) if !a.channels.is_empty() => {
                                s.resolve_bands(&a.bands, &a.channels, &mut scratch[i]);
                                &scratch[i]
                            }
                            _ => &a.bands,
                        };
                        let overall = a.overall * dev.spatial.as_ref().map_or(1.0, |s| s.distance_gain);
                        let engine = dev.engine.as_deref().unwrap_or(&global);
                        let mut color = engine.resolve(bands, overall);
                        if a.strobing && let Some(c) = strobe_color {
                            color = c;
                        }
                        if !gradients[i].is_empty() {
                            match (&dev.spatial, a.strobing && strobe_color.is_some()) {
                                (Some(s), false) if !a.channels.is_empty() => {
                                    for seg in 0..gradients[i].len() {
                                        s.resolve_segment_bands(seg, &a.bands, &a.channels, &mut scratch[i]);
                                        gradients[i][seg] = engine.resolve(&scratch[i], overall);
                                    }
                                }
                                _ => gradients[i].fill(color),
                            }
                        }
                        (to_frame(color, overall, a.transition_ms), a.force)
                    }
                    Msg::Silence => {
                        let base = last[i].unwrap_or(initial[i]);
                        (Frame { overall: silence_brightness, transition_ms: silence_fade_ms, ..base }, true)
                    }
                };

                if !force && !changed(&frame, last[i].as_ref(), deadband_rgb, deadband_overall) {
                    continue;
                }
                if let Err(e) = dev.apply(&frame, &gradients[i]) {
                    log_error!("{}: apply failed (ignored): {e:#}", dev.name());
                }
                last[i] = Some(frame);
                sent_any = true;
            }

            if sent_any {
                last_sent = Instant::now();
            }
        }
    }).context("spawn the output thread")
}

fn describe_bands(bands: &[BandConfig], engine: &ColorEngine) -> String {
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
            log_error!("{}: restore failed (ignored): {e:#}", dev.name());
        }
    }
}

static PANIC_HOOK: Once = Once::new();
static CURRENT_DEVICES: OnceLock<RwLock<Weak<Vec<RuntimeDevice>>>> = OnceLock::new();

fn current_devices() -> &'static RwLock<Weak<Vec<RuntimeDevice>>> { CURRENT_DEVICES.get_or_init(|| RwLock::new(Weak::new())) }
fn remember_devices(runtime: &Arc<Vec<RuntimeDevice>>) { *current_devices().write().unwrap_or_else(|e| e.into_inner()) = Arc::downgrade(runtime); }
fn forget_devices() {
    *current_devices().write().unwrap_or_else(|e| e.into_inner()) = Weak::new();
}

fn install_panic_hook() {
    PANIC_HOOK.call_once(|| {
        let previous = std::panic::take_hook();
        std::panic::set_hook(Box::new(move |info| {
            let live = current_devices().read().unwrap_or_else(|e| e.into_inner()).upgrade();
            if let Some(runtime) = live {
                restore_last_state(&runtime);
            }
            previous(info);
        }));
    });
}

fn lock<T>(m: &Mutex<T>) -> MutexGuard<'_, T> {
    m.lock().unwrap_or_else(|e| e.into_inner())
}
