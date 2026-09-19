use anyhow::{Context, Result, anyhow};
use serde::{Deserialize, Serialize};
use std::{collections::VecDeque, sync::{atomic::{AtomicBool, Ordering}, mpsc, Arc, Mutex, MutexGuard, }, thread, time::{Duration, Instant}};
use crate::config::{AppMatchMode, AudioDeviceSelector, AudioSection};
use crate::spatial::SpeakerLayout;
use crate::{log_error, log_info, log_warn};

#[cfg_attr(target_os = "windows", path = "windows.rs")]
#[cfg_attr(target_os = "macos", path = "macos.rs")]
#[cfg_attr(not(any(target_os = "windows", target_os = "macos")), path = "linux.rs")]
mod backend;

mod select;

pub trait CaptureStream { fn sample_rate(&self) -> f32;fn layout(&self) -> SpeakerLayout;fn read_frames(&mut self, out: &mut Vec<f32>) -> Result<usize>; }

/// The format a capture runs in. Device capture reads it off the endpoint; per-app capture has to state one, which is what this resolves.
#[derive(Debug, Clone, Copy)]
pub struct CaptureFormat {
    pub sample_rate: u32,
    pub channels: usize,
    pub channel_mask: u32, // Which speaker each channel stands for. `0` when the source does not say, and the count alone has to decide.
}

impl CaptureFormat {
    pub fn layout(&self) -> SpeakerLayout {
        match self.channel_mask {
            0 => SpeakerLayout::from_count(self.channels),
            mask => SpeakerLayout::from_channel_mask(mask, self.channels),
        }
    }
}
#[derive(Debug, Clone)]
pub struct AppTarget { pub pid: u32, pub label: String, pub gain: f32 }
#[derive(Debug, Clone, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS), ts(export))]
pub struct AppInfo {
    pub pid: u32,
    pub exe: String,
    pub display: String,
    pub playing: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS), ts(export))]
pub struct AudioDeviceInfo {
    pub id: String,
    pub name: String,
    pub layout: Option<String>,
    pub is_default: bool,
    pub is_loopback: bool,
}

const MIX_TICK: Duration = Duration::from_millis(10);
const SHUTDOWN_TICK: Duration = Duration::from_millis(200);
const MAX_APP_STREAMS: usize = 64; // One client per captured process, so this caps the threads a broadly matching pattern can start.
const SILENCE_EPSILON: f32 = 1e-6; // Below this a mixed chunk counts as digital silence and is not forwarded.
const DEAD_APP_GRACE: Duration = Duration::from_secs(2); // Silence from an app before its process is checked for being gone.
const FALLBACK_APP_FORMAT: CaptureFormat = CaptureFormat { sample_rate: 48_000, channels: 2, channel_mask: 0 }; // Used when the output device cannot be asked what it runs in.

pub fn enumerate_devices() -> Result<Vec<AudioDeviceInfo>> { off_thread("enumerate-devices", backend::enumerate_devices) }
pub fn enumerate_apps() -> Result<Vec<AppInfo>> {
    let mut apps = off_thread("enumerate-apps", backend::list_apps)?;
    apps.sort_by(|a, b| b.playing.cmp(&a.playing).then_with(|| a.display.to_lowercase().cmp(&b.display.to_lowercase())));
    Ok(apps)
}

fn off_thread<T: Send + 'static>(name: &str, f: impl FnOnce() -> Result<T> + Send + 'static) -> Result<T> {
    let handle = thread::Builder::new().name(name.to_string()).spawn(f).with_context(|| format!("spawn the {name} thread"))?;
    handle.join().map_err(|_| anyhow!("the {name} thread panicked"))?
}

pub struct Capture {
    rx: mpsc::Receiver<Vec<f32>>,
    sample_rate: f32,
    layout: SpeakerLayout,
    source: String,
    handles: Vec<thread::JoinHandle<()>>,
}

impl Capture {
    //noinspection RsConstantConditionIf
    pub fn start(audio: &AudioSection, running: Arc<AtomicBool>, warnings: &mut Vec<String>) -> Result<Capture> {
        if audio.apps.is_usable(warnings) {
            if backend::APP_CAPTURE_SUPPORTED {
                return Ok(start_apps(audio, running, warnings));
            }
            warnings.push("audio.apps.enabled is true but capturing a single application is only implemented on Windows, capturing the output device instead".to_string());
        }
        start_device(audio, running)
    }
    pub fn sample_rate(&self) -> f32 {
        self.sample_rate
    }
    pub fn channels(&self) -> usize {
        self.layout.len()
    }
    pub fn layout(&self) -> &SpeakerLayout {
        &self.layout
    }
    pub fn source(&self) -> &str {
        &self.source
    }
    pub fn recv_timeout(&self, timeout: Duration) -> Result<Vec<f32>, mpsc::RecvTimeoutError> { self.rx.recv_timeout(timeout) }
    pub fn stop(self) {
        let Capture { rx, handles, .. } = self;
        drop(rx);
        for handle in handles {
            let _ = handle.join();
        }
    }
}

struct StreamInfo { sample_rate: f32, layout: SpeakerLayout, }

fn start_device(audio: &AudioSection, running: Arc<AtomicBool>) -> Result<Capture> {
    let (tx, rx) = mpsc::channel::<Vec<f32>>();
    let (ready_tx, ready_rx) = mpsc::channel::<Result<StreamInfo>>();

    let cfg = audio.clone();
    let handle = thread::Builder::new().name("capture-device".to_string()).spawn(move || {
        let mut stream = match backend::open_device(&cfg.device, &cfg) {
            Ok(s) => {
                let _ = ready_tx.send(Ok(StreamInfo { sample_rate: s.sample_rate(), layout: s.layout() }));
                s
            }
            Err(e) => {
                let _ = ready_tx.send(Err(e));
                return;
            }
        };
        drop(ready_tx);
        pump(&mut *stream, &tx, &running);
    })?;

    let info = ready_rx.recv().context("capture thread died before it was ready")??;
    Ok(Capture { rx, sample_rate: info.sample_rate, layout: info.layout, source: describe_device(&audio.device), handles: vec![handle] })
}

fn describe_device(sel: &AudioDeviceSelector) -> String {
    match sel {
        AudioDeviceSelector::Default => "default output device".to_string(),
        AudioDeviceSelector::Id { id } => format!("output device {id}"),
        AudioDeviceSelector::Name { name } => format!("output device {name:?}"),
    }
}

fn pump(stream: &mut dyn CaptureStream, tx: &mpsc::Sender<Vec<f32>>, running: &AtomicBool) {
    let mut buf = Vec::new();
    while running.load(Ordering::SeqCst) {
        buf.clear();
        match stream.read_frames(&mut buf) {
            Ok(0) => continue,
            Ok(_) => {
                if tx.send(std::mem::take(&mut buf)).is_err() {
                    return;
                }
            }
            Err(e) => {
                log_error!("Capture: stream ended: {e:#}");
                return;
            }
        }
    }
}

struct Slot {
    pid: u32,
    gain: f32,
    buf: Mutex<VecDeque<f32>>,
    alive: AtomicBool,
}

/**
 * What per-app capture records in.
 *
 * There is no mix format to ask for the way an endpoint has one, so the format is either stated in the config or taken from the output device — which
 * is what the applications are playing into anyway. Taking it from the device is what keeps a 7.1 setup from being recorded as stereo.
 */
fn app_format(audio: &AudioSection, warnings: &mut Vec<String>) -> CaptureFormat {
    let stated = (audio.apps.sample_rate, audio.apps.channels);
    let device = match backend::output_format(&audio.device) {
        Ok(format) => Some(format),
        Err(e) => {
            if stated.0.is_none() || stated.1.is_none() {
                warnings.push(format!("could not read the format of the output device ({e:#}), per-app capture falls back to {} Hz and {} channels", FALLBACK_APP_FORMAT.sample_rate, FALLBACK_APP_FORMAT.channels));
            }
            None
        }
    };
    let fallback = device.unwrap_or(FALLBACK_APP_FORMAT);

    let channels = stated.1.map_or(fallback.channels, |c| c as usize).clamp(1, 8);
    // Saying so beats leaving someone to wonder why a 7.1 output shows up as stereo in the status line.
    if let (Some(_), Some(device)) = (stated.1, device) && channels != device.channels {
        warnings.push(format!("audio.apps.channels is {channels} while the output device runs {} channels, so the capture is {channels} channel(s). Remove the setting to follow the device.", device.channels));
    }
    CaptureFormat {
        sample_rate: stated.0.unwrap_or(fallback.sample_rate).max(8_000),
        channels,
        // The mask describes the device's own channels, so it only fits while the count is the device's too.
        channel_mask: if channels == fallback.channels { fallback.channel_mask } else { 0 },
    }
}

fn start_apps(audio: &AudioSection, running: Arc<AtomicBool>, warnings: &mut Vec<String>) -> Capture {
    let (tx, rx) = mpsc::channel::<Vec<f32>>();
    let apps = audio.apps.clone();
    let format = app_format(audio, warnings);
    let sample_rate = format.sample_rate as f32;
    let channels = format.channels;
    let live: Arc<Mutex<Vec<Arc<Slot>>>> = Arc::new(Mutex::new(Vec::new()));
    let mut handles = Vec::with_capacity(2);
    {
        let live = Arc::clone(&live);
        let running = Arc::clone(&running);
        let capacity = sample_rate as usize * channels; // One second per app is plenty; older samples are dropped.
        let audio = audio.clone();
        handles.extend(thread::Builder::new().name("capture-apps".to_string()).spawn(move || { supervise(audio, format, live, running, capacity); }));
    }
    {
        let running = Arc::clone(&running);
        let max_chunk = (sample_rate * 0.1) as usize * channels; // Never hand out more than 100 ms at once.
        handles.extend(thread::Builder::new().name("capture-mixer".to_string()).spawn(move || { mix(live, tx, running, max_chunk, channels); }));
    }
    let mut patterns: Vec<String> = Vec::new();
    for g in apps.active_groups() {
        patterns.extend(g.apps.iter().filter(|p| !p.trim().is_empty()).cloned());
    }
    Capture { rx, sample_rate, layout: format.layout(), source: match apps.mode {
        AppMatchMode::Include => format!("apps {}", patterns.join(", ")),
        AppMatchMode::Exclude => format!("every app except {}", patterns.join(", ")),
    }, handles }
}

fn supervise(audio: AudioSection, format: CaptureFormat, live: Arc<Mutex<Vec<Arc<Slot>>>>, running: Arc<AtomicBool>, capacity: usize) {
    let mut announced_failure = false;
    let mut announced_limit = false;
    let mut children: Vec<thread::JoinHandle<()>> = Vec::new();

    while running.load(Ordering::SeqCst) {
        lock(&live).retain(|s| s.alive.load(Ordering::SeqCst));
        children.retain(|h| !h.is_finished());

        match backend::resolve_targets(&audio.apps) {
            Ok(targets) => {
                announced_failure = false;
                for target in targets {
                    if lock(&live).iter().any(|s| s.pid == target.pid) {
                        continue;
                    }
                    if lock(&live).len() >= MAX_APP_STREAMS {
                        if !announced_limit {
                            log_warn!("Capture: not capturing more than {MAX_APP_STREAMS} processes at once, narrow audio.apps.groups[].apps down.");
                            announced_limit = true;
                        }
                        break;
                    }
                    let slot = Arc::new(Slot {
                        pid: target.pid,
                        gain: target.gain,
                        buf: Mutex::new(VecDeque::with_capacity(capacity)),
                        alive: AtomicBool::new(true),
                    });
                    lock(&live).push(Arc::clone(&slot));

                    let pid = target.pid;
                    let running = Arc::clone(&running);
                    let owned = Arc::clone(&slot);
                    let spawned = thread::Builder::new().name(format!("capture-pid-{pid}")).spawn(move || {
                        capture_app(&target, format, &owned, &running, capacity);
                        owned.alive.store(false, Ordering::SeqCst);
                    });
                    match spawned {
                        Ok(handle) => children.push(handle),
                        Err(e) => {
                            log_error!("Capture: could not start a capture thread for pid {pid}: {e}");
                            slot.alive.store(false, Ordering::SeqCst);
                        }
                    }
                }
            }
            Err(e) => {
                if !announced_failure {
                    log_error!("Capture: could not list applications: {e:#}");
                    announced_failure = true;
                }
            }
        }

        sleep_interruptible(Duration::from_millis(audio.apps.rescan_ms.max(250)), &running);
    }

    for child in children {
        let _ = child.join();
    }
}

fn capture_app(target: &AppTarget, format: CaptureFormat, slot: &Slot, running: &AtomicBool, capacity: usize) {
    let channels = format.channels;
    let mut stream = match backend::open_app(target, &format) {
        Ok(s) => s,
        Err(e) => {
            log_error!("Capture: cannot capture {} (pid {}): {e:#}", target.label, target.pid);
            return;
        }
    };

    let mut buf = Vec::new();
    let mut last_data = Instant::now();
    let mut announced = false;
    while running.load(Ordering::SeqCst) {
        buf.clear();
        match stream.read_frames(&mut buf) {
            Ok(0) => {
                if last_data.elapsed() > DEAD_APP_GRACE && !backend::process_alive(target.pid) {
                    break;
                }
            }
            Ok(_) => {
                last_data = Instant::now();
                if !announced && buf.iter().any(|s| s.abs() > SILENCE_EPSILON) {
                    log_info!("Capture: {} (pid {})", target.label, target.pid);
                    announced = true;
                }
                let mut q = lock(&slot.buf);
                let overflow = ((q.len() + buf.len()).saturating_sub(capacity).min(q.len()).div_ceil(channels) * channels).min(q.len());
                q.drain(..overflow);
                q.extend(buf.iter().copied());
            }
            Err(e) => {
                log_warn!("Capture: {} (pid {}) ended: {e:#}", target.label, target.pid);
                break;
            }
        }
    }
    if announced {
        log_info!("Capture: stopped capturing {} (pid {})", target.label, target.pid);
    }
}

fn mix(live: Arc<Mutex<Vec<Arc<Slot>>>>, tx: mpsc::Sender<Vec<f32>>, running: Arc<AtomicBool>, max_chunk: usize, channels: usize) {
    let channels = channels.max(1);
    let max_frames = (max_chunk.max(64) / channels).max(1);

    while running.load(Ordering::SeqCst) {
        thread::sleep(MIX_TICK);

        let slots: Vec<Arc<Slot>> = lock(&live).clone();
        if slots.is_empty() {
            continue;
        }

        let frames = slots.iter().map(|s| lock(&s.buf).len() / channels).max().unwrap_or(0).min(max_frames);
        if frames == 0 {
            continue;
        }
        let mut mixed = vec![0.0f32; frames * channels];
        let mut loudest = 0.0f32;
        for slot in &slots {
            let mut q = lock(&slot.buf);
            let take = (q.len() / channels).min(frames) * channels;
            for (out, s) in mixed.iter_mut().zip(q.drain(..take)) {
                *out += s * slot.gain;
            }
        }
        for s in mixed.iter_mut() {
            *s = s.clamp(-1.0, 1.0);
            loudest = loudest.max(s.abs());
        }
        if loudest < SILENCE_EPSILON {
            continue;
        }
        if tx.send(mixed).is_err() {
            return;
        }
    }
}

fn lock<T>(m: &Mutex<T>) -> MutexGuard<'_, T> {
    m.lock().unwrap_or_else(|e| e.into_inner())
}

fn sleep_interruptible(total: Duration, running: &AtomicBool) {
    let mut left = total;
    while left > Duration::ZERO && running.load(Ordering::SeqCst) {
        let step = left.min(SHUTDOWN_TICK);
        thread::sleep(step);
        left -= step;
    }
}
