use anyhow::{anyhow, Context, Result};
use std::{
    collections::{HashMap, HashSet},
    ptr,
    sync::{Mutex, OnceLock},
};
use wasapi::{
    initialize_mta, AudioCaptureClient, AudioClient, Device, DeviceEnumerator, Direction, Handle, SampleType, SessionState, StreamMode, WaveFormat,
};
use windows::{
    core::{PCWSTR, PWSTR},
    Win32::{
        Foundation::{CloseHandle, HANDLE, MAX_PATH},
        Storage::FileSystem::{GetFileVersionInfoSizeW, GetFileVersionInfoW, VerQueryValueW},
        System::{
            Diagnostics::ToolHelp::{CreateToolhelp32Snapshot, Process32FirstW, Process32NextW, PROCESSENTRY32W, TH32CS_SNAPPROCESS},
            Threading::{GetExitCodeProcess, OpenProcess, QueryFullProcessImageNameW, PROCESS_NAME_WIN32, PROCESS_QUERY_LIMITED_INFORMATION},
        },
    },
};

use crate::audio::{cast_slice, downmix};
use crate::capture::{AppInfo, AppTarget, CaptureStream};
use crate::config::{AppGroup, AppMatchMode, AppsSection, AudioDeviceSelector, AudioSection, Downmix};

const STILL_RUNNING: u32 = 259; // STILL_ACTIVE, the exit code of a process that has not exited.
const READ_TIMEOUT_MS: u32 = 100; // Kept short so a capture thread notices Ctrl+C quickly.

struct WasapiStream {
    _client: AudioClient, // Owns the stream; dropping it would stop the capture client below.
    capture: AudioCaptureClient,
    event: Handle,
    channels: usize,
    bytes_per_frame: usize,
    sample_rate: f32,
    downmix: Downmix,
    raw: Vec<u8>,
}

impl CaptureStream for WasapiStream {
    fn sample_rate(&self) -> f32 {
        self.sample_rate
    }

    fn read_mono(&mut self, out: &mut Vec<f32>) -> Result<usize> {
        if self.event.wait_for_event(READ_TIMEOUT_MS).is_err() {
            return Ok(0); // Nothing was delivered in time. Silence is decided further up.
        }

        let before = out.len();
        while let Some(frames) = self.capture.get_next_packet_size()? {
            if frames == 0 {
                break;
            }
            let wanted = frames as usize * self.bytes_per_frame;
            if self.raw.len() < wanted {
                self.raw.resize(wanted, 0);
            }
            let (read_frames, _info) = self.capture.read_from_device(&mut self.raw[..wanted])?;
            if read_frames == 0 {
                break;
            }
            let floats: &[f32] = cast_slice(&self.raw[..read_frames as usize * self.bytes_per_frame]);
            out.extend(floats.chunks_exact(self.channels).map(|frame| downmix(frame, self.downmix)));
        }
        Ok(out.len() - before)
    }
}

fn stream_from(client: AudioClient, format: &WaveFormat, downmix_mode: Downmix) -> Result<Box<dyn CaptureStream>> {
    let capture = client.get_audiocaptureclient()?;
    let event = client.set_get_eventhandle()?;
    client.start_stream()?;

    let channels = format.get_nchannels() as usize;
    Ok(Box::new(WasapiStream {
        capture,
        event,
        channels,
        bytes_per_frame: channels * size_of::<f32>(),
        sample_rate: format.get_samplespersec().max(1) as f32,
        downmix: downmix_mode,
        raw: Vec::new(),
        _client: client,
    }))
}

pub fn open_device(sel: &AudioDeviceSelector, audio: &AudioSection) -> Result<Box<dyn CaptureStream>> {
    initialize_mta().ok().context("initialize_mta failed (COM init; avoid calling from STA UI thread)")?;
    let enumerator = DeviceEnumerator::new()?;

    match sel {
        AudioDeviceSelector::Id { id } if id.is_empty() => {
            list_render_devices(&enumerator);
            return Err(anyhow!("audio.device id must not be empty"));
        }
        AudioDeviceSelector::Name { name } if name.is_empty() => {
            list_render_devices(&enumerator);
            return Err(anyhow!("audio.device name must not be empty"));
        }
        _ => {}
    }

    let device = match select_render_device(&enumerator, sel) {
        Ok(d) => d,
        Err(e) => {
            list_render_devices(&enumerator);
            return Err(e);
        }
    };

    let mut client = device.get_iaudioclient()?;
    let format = client.get_mixformat()?;
    let mode = StreamMode::EventsShared { autoconvert: true, buffer_duration_hns: audio.buffer_duration_hns };
    client.initialize_client(&format, &Direction::Capture, &mode)?;
    stream_from(client, &format, audio.downmix)
}

pub fn list_devices() {
    if initialize_mta().ok().is_err() {
        eprintln!("COM could not be initialized, cannot list the audio devices.");
        return;
    }
    let Ok(enumerator) = DeviceEnumerator::new() else {
        eprintln!("Could not open the audio device enumerator.");
        return;
    };
    list_render_devices(&enumerator);
}

fn list_render_devices(enumerator: &DeviceEnumerator) {
    if let Ok(coll) = enumerator.get_device_collection(&Direction::Render) {
        eprintln!("--- Render devices (Output) ---");
        for dev_res in &coll {
            let Ok(dev) = dev_res else { continue };
            let name = dev.get_friendlyname().unwrap_or_else(|_| "<no name>".to_string());
            let id = dev.get_id().unwrap_or_else(|_| "<no id>".to_string());
            eprintln!("  - {name}\n    id: {id}");
        }
    }
}

fn select_render_device(enumerator: &DeviceEnumerator, sel: &AudioDeviceSelector) -> Result<Device> {
    match sel {
        AudioDeviceSelector::Default => Ok(enumerator.get_default_device(&Direction::Render)?),
        AudioDeviceSelector::Id { id } => Ok(enumerator.get_device(id)?),
        AudioDeviceSelector::Name { name } => {
            let coll = enumerator.get_device_collection(&Direction::Render)?;
            for dev_res in &coll {
                let dev = dev_res?;
                if dev.get_friendlyname().unwrap_or_default().to_lowercase().contains(&name.to_lowercase()) {
                    return Ok(dev);
                }
            }
            anyhow::bail!("Audio device not found by name: {name}");
        }
    }
}

pub fn open_app(target: &AppTarget, audio: &AudioSection) -> Result<Box<dyn CaptureStream>> {
    initialize_mta().ok().context("initialize_mta failed (COM init; avoid calling from STA UI thread)")?;
    let apps = &audio.apps;
    let format = WaveFormat::new(32, 32, &SampleType::Float, apps.sample_rate.max(8_000) as usize, apps.channels.clamp(1, 8) as usize, None);
    let mut client = AudioClient::new_application_loopback_client(target.pid, false).map_err(|e| anyhow!("process loopback capture needs Windows 10 2004 or newer: {e}"))?;
    let mode = StreamMode::EventsShared { autoconvert: true, buffer_duration_hns: 0 };
    client.initialize_client(&format, &Direction::Capture, &mode)?;

    stream_from(client, &format, audio.downmix)
}

pub fn process_alive(pid: u32) -> bool {
    let Ok(handle) = (unsafe { OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, false, pid) }) else {
        return false;
    };
    let mut code = 0u32;
    let alive = unsafe { GetExitCodeProcess(handle, &mut code) }.is_ok() && code == STILL_RUNNING;
    close(handle);
    alive
}

pub fn resolve_targets(apps: &AppsSection) -> Result<Vec<AppTarget>> {
    initialize_mta().ok().context("initialize_mta failed (COM init; avoid calling from STA UI thread)")?;

    let table = process_table()?;
    let parents: HashMap<u32, u32> = table.iter().map(|p| (p.pid, p.parent)).collect();
    let own = std::process::id();

    let label_of = |pid: u32| table.iter().find(|p| p.pid == pid).map(|p| p.display.clone()).unwrap_or_else(|| format!("pid {pid}"));

    let mut targets: Vec<AppTarget> = Vec::new();
    match apps.mode {
        AppMatchMode::Include => {
            for group in apps.active_groups() {
                let matched: HashSet<u32> = table.iter().filter(|p| matches_group(group, p)).map(|p| p.pid).collect();

                for proc in &table {
                    if proc.pid == 0 || proc.pid == own {
                        continue;
                    }
                    if !matched.contains(&proc.pid) && !(apps.include_process_tree && has_ancestor_in(proc.pid, &matched, &parents)) {
                        continue;
                    }
                    push_target(&mut targets, AppTarget { pid: proc.pid, label: proc.display.clone(), gain: group.gain });
                }
            }
        }
        AppMatchMode::Exclude => {
            let mut excluded: HashSet<u32> = HashSet::new();
            for group in apps.active_groups() {
                excluded.extend(table.iter().filter(|p| matches_group(group, p)).map(|p| p.pid));
            }

            for (pid, _playing) in render_session_pids()? {
                if pid == 0 || pid == own || excluded.contains(&pid) || has_ancestor_in(pid, &excluded, &parents) {
                    continue;
                }
                push_target(&mut targets, AppTarget { pid, label: label_of(pid), gain: 1.0 });
            }
        }
    }
    Ok(targets)
}

fn push_target(targets: &mut Vec<AppTarget>, target: AppTarget) {
    if !targets.iter().any(|t| t.pid == target.pid) {
        targets.push(target);
    }
}

fn has_ancestor_in(pid: u32, set: &HashSet<u32>, parents: &HashMap<u32, u32>) -> bool {
    let mut current = pid;
    for _ in 0..64 {
        let Some(&parent) = parents.get(&current) else { return false };
        if parent == 0 || parent == current {
            return false;
        }
        if set.contains(&parent) {
            return true;
        }
        current = parent;
    }
    false
}

fn matches_group(group: &AppGroup, proc: &ProcEntry) -> bool {
    group.apps.iter().any(|pattern| matches_pattern(pattern, proc))
}

fn matches_pattern(pattern: &str, proc: &ProcEntry) -> bool {
    let pattern = pattern.trim().to_lowercase();
    if pattern.is_empty() {
        return false;
    }
    let exe = proc.exe.to_lowercase();
    exe == pattern || exe.strip_suffix(".exe").unwrap_or(&exe) == pattern || proc.display.to_lowercase().contains(&pattern)
}

pub fn list_apps() -> Result<Vec<AppInfo>> {
    initialize_mta().ok().context("initialize_mta failed (COM init; avoid calling from STA UI thread)")?;

    let table = process_table()?;
    let mut out: Vec<AppInfo> = Vec::new();

    for (pid, playing) in render_session_pids()? {
        if out.iter().any(|a| a.pid == pid) {
            continue;
        }
        if pid == 0 {
            out.push(AppInfo { pid, exe: "-".to_string(), display: "System sounds (cannot be captured)".to_string(), playing });
            continue;
        }
        let Some(proc) = table.iter().find(|p| p.pid == pid) else { continue }; // The process is already gone.
        out.push(AppInfo { pid, exe: proc.exe.clone(), display: proc.display.clone(), playing });
    }
    Ok(out)
}

fn render_session_pids() -> Result<Vec<(u32, bool)>> {
    let enumerator = DeviceEnumerator::new()?;
    let collection = enumerator.get_device_collection(&Direction::Render)?;

    let mut out: Vec<(u32, bool)> = Vec::new();
    for dev_res in &collection {
        let Ok(dev) = dev_res else { continue };
        let Ok(manager) = dev.get_iaudiosessionmanager() else { continue };
        let Ok(sessions) = manager.get_audiosessionenumerator() else { continue };
        let Ok(count) = sessions.get_count() else { continue };

        for i in 0..count {
            let Ok(session) = sessions.get_session(i) else { continue };
            let Ok(pid) = session.get_process_id() else { continue };
            let playing = matches!(session.get_state(), Ok(SessionState::Active));

            match out.iter_mut().find(|(p, _)| *p == pid) {
                Some(entry) => entry.1 |= playing,
                None => out.push((pid, playing)),
            }
        }
    }
    Ok(out)
}

struct ProcEntry {
    pid: u32,
    parent: u32,
    exe: String,     // `chrome.exe`
    display: String, // `Google Chrome`, the file description the volume mixer shows.
}

fn process_table() -> Result<Vec<ProcEntry>> {
    let snapshot = unsafe { CreateToolhelp32Snapshot(TH32CS_SNAPPROCESS, 0) }.context("CreateToolhelp32Snapshot")?;

    let mut entry = PROCESSENTRY32W { dwSize: size_of::<PROCESSENTRY32W>() as u32, ..Default::default() };
    let mut out = Vec::new();
    if unsafe { Process32FirstW(snapshot, &mut entry) }.is_ok() {
        loop {
            let exe = wide_to_string(&entry.szExeFile);
            out.push(ProcEntry {
                pid: entry.th32ProcessID,
                parent: entry.th32ParentProcessID,
                display: display_name(entry.th32ProcessID, &exe),
                exe,
            });
            if unsafe { Process32NextW(snapshot, &mut entry) }.is_err() {
                break;
            }
        }
    }
    close(snapshot);
    Ok(out)
}

fn display_name(pid: u32, exe: &str) -> String {
    let fallback = || {
        let stem = exe.strip_suffix(".exe").or_else(|| exe.strip_suffix(".EXE")).unwrap_or(exe);
        stem.to_string()
    };

    let Some(path) = executable_path(pid) else { return fallback() };

    let mut cache = descriptions().lock().unwrap_or_else(|e| e.into_inner());
    if let Some(hit) = cache.get(&path) {
        return if hit.is_empty() { fallback() } else { hit.clone() };
    }

    let found = file_description(&path).unwrap_or_default();
    cache.insert(path, found.clone());
    if found.is_empty() { fallback() } else { found }
}

fn descriptions() -> &'static Mutex<HashMap<String, String>> {
    static CACHE: OnceLock<Mutex<HashMap<String, String>>> = OnceLock::new();
    CACHE.get_or_init(|| Mutex::new(HashMap::new()))
}

fn executable_path(pid: u32) -> Option<String> {
    let handle = unsafe { OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, false, pid) }.ok()?;

    let mut buf = vec![0u16; MAX_PATH as usize * 2];
    let mut len = buf.len() as u32;
    let result = unsafe { QueryFullProcessImageNameW(handle, PROCESS_NAME_WIN32, PWSTR(buf.as_mut_ptr()), &mut len) };
    close(handle);

    result.ok()?;
    Some(String::from_utf16_lossy(&buf[..len as usize]))
}

fn file_description(path: &str) -> Option<String> {
    let path_w = wide(path);
    let size = unsafe { GetFileVersionInfoSizeW(PCWSTR(path_w.as_ptr()), None) };
    if size == 0 {
        return None;
    }

    let mut data = vec![0u8; size as usize];
    unsafe { GetFileVersionInfoW(PCWSTR(path_w.as_ptr()), None, size, data.as_mut_ptr() as *mut _) }.ok()?;

    let key = wide("\\VarFileInfo\\Translation");
    let mut block: *mut core::ffi::c_void = ptr::null_mut();
    let mut block_len = 0u32;
    let found = unsafe { VerQueryValueW(data.as_ptr() as *const _, PCWSTR(key.as_ptr()), &mut block, &mut block_len) };
    if !found.as_bool() || block_len < 4 {
        return None;
    }
    let (language, codepage) = unsafe { (*(block as *const u16), *(block as *const u16).add(1)) };

    let key = wide(&format!("\\StringFileInfo\\{language:04x}{codepage:04x}\\FileDescription"));
    let mut text: *mut core::ffi::c_void = ptr::null_mut();
    let mut text_len = 0u32;
    let found = unsafe { VerQueryValueW(data.as_ptr() as *const _, PCWSTR(key.as_ptr()), &mut text, &mut text_len) };
    if !found.as_bool() || text_len == 0 {
        return None;
    }
    let value = wide_to_string(unsafe { std::slice::from_raw_parts(text as *const u16, text_len as usize) });
    if value.is_empty() { None } else { Some(value) }
}

fn close(handle: HANDLE) {
    let _ = unsafe { CloseHandle(handle) };
}

fn wide(s: &str) -> Vec<u16> {
    s.encode_utf16().chain(std::iter::once(0)).collect()
}

fn wide_to_string(chars: &[u16]) -> String {
    let end = chars.iter().position(|&c| c == 0).unwrap_or(chars.len());
    String::from_utf16_lossy(&chars[..end]).trim().to_string()
}
