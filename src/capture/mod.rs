use anyhow::{Context, Result};
use std::{
    collections::VecDeque,
    sync::{
        atomic::{AtomicBool, Ordering},
        mpsc, Arc, Mutex, MutexGuard,
    },
    thread,
    time::{Duration, Instant},
};

use crate::config::{AppMatchMode, AudioDeviceSelector, AudioSection};

#[cfg_attr(target_os = "windows", path = "windows.rs")]
#[cfg_attr(target_os = "macos", path = "macos.rs")]
#[cfg_attr(not(any(target_os = "windows", target_os = "macos")), path = "linux.rs")]
mod backend;

pub trait CaptureStream {
    fn sample_rate(&self) -> f32;
    fn read_mono(&mut self, out: &mut Vec<f32>) -> Result<usize>;
}
#[derive(Debug, Clone)]
pub struct AppTarget {
    pub pid: u32,
    pub label: String, // Display name, for log output.
    pub gain: f32,
}
#[derive(Debug, Clone)]
pub struct AppInfo {
    pub pid: u32,
    pub exe: String,
    pub display: String,
    pub playing: bool,
}

const MIX_TICK: Duration = Duration::from_millis(10);
const SHUTDOWN_TICK: Duration = Duration::from_millis(200);
const MAX_APP_STREAMS: usize = 64; // One client per captured process, so this caps the threads a broadly matching pattern can start.
const SILENCE_EPSILON: f32 = 1e-6; // Below this a mixed chunk counts as digital silence and is not forwarded.
const DEAD_APP_GRACE: Duration = Duration::from_secs(2); // Silence from an app before its process is checked for being gone.

// ---------------------------------------------------------------- listing

pub fn list_devices() {
    backend::list_devices();
}

pub fn print_apps() -> Result<()> {
    let mut apps = backend::list_apps()?;
    apps.sort_by(|a, b| b.playing.cmp(&a.playing).then_with(|| a.display.to_lowercase().cmp(&b.display.to_lowercase())));

    if apps.is_empty() {
        eprintln!("--- Applications ---");
        eprintln!("  (none found)");
        return Ok(());
    }

    let width = apps.iter().map(|a| a.display.chars().count()).max().unwrap_or(0);
    eprintln!("--- Applications (usable in audio.apps.groups[].apps) ---");
    for a in &apps {
        eprintln!("  {:<width$}  {:<24} pid {:<8} {}", a.display, a.exe, a.pid, if a.playing { "playing" } else { "idle" });
    }
    Ok(())
}

pub struct Capture {
    rx: mpsc::Receiver<Vec<f32>>,
    sample_rate: f32,
    source: String,
}

impl Capture {
    pub fn start(audio: &AudioSection, running: Arc<AtomicBool>, warnings: &mut Vec<String>) -> Result<Capture> {
        if audio.apps.is_usable(warnings) {
            Ok(start_apps(audio, running))
        } else {
            start_device(audio, running)
        }
    }
    pub fn sample_rate(&self) -> f32 {
        self.sample_rate
    }
    pub fn source(&self) -> &str {
        &self.source
    }
    pub fn recv_timeout(&self, timeout: Duration) -> Result<Vec<f32>, mpsc::RecvTimeoutError> {
        self.rx.recv_timeout(timeout)
    }
}

fn start_device(audio: &AudioSection, running: Arc<AtomicBool>) -> Result<Capture> {
    let (tx, rx) = mpsc::channel::<Vec<f32>>();
    let (ready_tx, ready_rx) = mpsc::channel::<Result<f32>>();

    let cfg = audio.clone();
    thread::Builder::new().name("capture-device".to_string()).spawn(move || {
        let mut stream = match backend::open_device(&cfg.device, &cfg) {
            Ok(s) => {
                let _ = ready_tx.send(Ok(s.sample_rate()));
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

    let sample_rate = ready_rx.recv().context("capture thread died before it was ready")??;
    Ok(Capture { rx, sample_rate, source: describe_device(&audio.device) })
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
        match stream.read_mono(&mut buf) {
            Ok(0) => continue,
            Ok(_) => {
                if tx.send(std::mem::take(&mut buf)).is_err() {
                    return;
                }
            }
            Err(e) => {
                eprintln!("Capture: stream ended: {e:#}");
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

fn start_apps(audio: &AudioSection, running: Arc<AtomicBool>) -> Capture {
    let (tx, rx) = mpsc::channel::<Vec<f32>>();
    let apps = audio.apps.clone();
    let sample_rate = apps.sample_rate.max(8_000) as f32;
    let live: Arc<Mutex<Vec<Arc<Slot>>>> = Arc::new(Mutex::new(Vec::new()));
    {
        let live = Arc::clone(&live);
        let running = Arc::clone(&running);
        let capacity = sample_rate as usize; // One second per app is plenty; older samples are dropped.
        let audio = audio.clone();
        let _ = thread::Builder::new().name("capture-apps".to_string()).spawn(move || { supervise(audio, live, running, capacity); });
    }
    {
        let running = Arc::clone(&running);
        let max_chunk = (sample_rate * 0.1) as usize; // Never hand out more than 100 ms at once.
        let _ = thread::Builder::new().name("capture-mixer".to_string()).spawn(move || { mix(live, tx, running, max_chunk); });
    }
    let mut patterns: Vec<String> = Vec::new();
    for g in apps.active_groups() {
        patterns.extend(g.apps.iter().filter(|p| !p.trim().is_empty()).cloned());
    }
    Capture { rx, sample_rate, source: match apps.mode {
        AppMatchMode::Include => format!("apps {}", patterns.join(", ")),
        AppMatchMode::Exclude => format!("every app except {}", patterns.join(", ")),
    }}
}

fn supervise(audio: AudioSection, live: Arc<Mutex<Vec<Arc<Slot>>>>, running: Arc<AtomicBool>, capacity: usize) {
    let mut announced_failure = false;
    let mut announced_limit = false;

    while running.load(Ordering::SeqCst) {
        lock(&live).retain(|s| s.alive.load(Ordering::SeqCst));

        match backend::resolve_targets(&audio.apps) {
            Ok(targets) => {
                announced_failure = false;
                for target in targets {
                    if lock(&live).iter().any(|s| s.pid == target.pid) {
                        continue;
                    }
                    if lock(&live).len() >= MAX_APP_STREAMS {
                        if !announced_limit {
                            eprintln!("Capture: not capturing more than {MAX_APP_STREAMS} processes at once, narrow audio.apps.groups[].apps down.");
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
                    let audio = audio.clone();
                    let running = Arc::clone(&running);
                    let owned = Arc::clone(&slot);
                    let spawned = thread::Builder::new().name(format!("capture-pid-{pid}")).spawn(move || {
                        capture_app(&target, &audio, &owned, &running, capacity);
                        owned.alive.store(false, Ordering::SeqCst);
                    });
                    if let Err(e) = spawned {
                        eprintln!("Capture: could not start a capture thread for pid {pid}: {e}");
                        slot.alive.store(false, Ordering::SeqCst);
                    }
                }
            }
            Err(e) => {
                if !announced_failure {
                    eprintln!("Capture: could not list applications: {e:#}");
                    announced_failure = true;
                }
            }
        }

        sleep_interruptible(Duration::from_millis(audio.apps.rescan_ms.max(250)), &running);
    }
}

fn capture_app(target: &AppTarget, audio: &AudioSection, slot: &Slot, running: &AtomicBool, capacity: usize) {
    let mut stream = match backend::open_app(target, audio) {
        Ok(s) => s,
        Err(e) => {
            eprintln!("Capture: cannot capture {} (pid {}): {e:#}", target.label, target.pid);
            return;
        }
    };

    let mut buf = Vec::new();
    let mut last_data = Instant::now();
    let mut announced = false;
    while running.load(Ordering::SeqCst) {
        buf.clear();
        match stream.read_mono(&mut buf) {
            Ok(0) => {
                if last_data.elapsed() > DEAD_APP_GRACE && !backend::process_alive(target.pid) {
                    break;
                }
            }
            Ok(_) => {
                last_data = Instant::now();
                if !announced && buf.iter().any(|s| s.abs() > SILENCE_EPSILON) {
                    eprintln!("Capture: {} (pid {})", target.label, target.pid);
                    announced = true;
                }
                let mut q = lock(&slot.buf);
                let overflow = (q.len() + buf.len()).saturating_sub(capacity).min(q.len());
                q.drain(..overflow);
                q.extend(buf.iter().copied());
            }
            Err(e) => {
                eprintln!("Capture: {} (pid {}) ended: {e:#}", target.label, target.pid);
                break;
            }
        }
    }
    if announced {
        eprintln!("Capture: stopped capturing {} (pid {})", target.label, target.pid);
    }
}

fn mix(live: Arc<Mutex<Vec<Arc<Slot>>>>, tx: mpsc::Sender<Vec<f32>>, running: Arc<AtomicBool>, max_chunk: usize) {
    let max_chunk = max_chunk.max(64);

    while running.load(Ordering::SeqCst) {
        thread::sleep(MIX_TICK);

        let slots: Vec<Arc<Slot>> = lock(&live).clone();
        if slots.is_empty() {
            continue;
        }

        let frames = slots.iter().map(|s| lock(&s.buf).len()).max().unwrap_or(0).min(max_chunk);
        if frames == 0 {
            continue;
        }
        let mut mixed = vec![0.0f32; frames];
        let mut loudest = 0.0f32;
        for slot in &slots {
            let mut q = lock(&slot.buf);
            for out in mixed.iter_mut() {
                match q.pop_front() {
                    Some(s) => *out += s * slot.gain,
                    None => break,
                }
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
