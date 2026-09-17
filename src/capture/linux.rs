use anyhow::{bail, Result};

use crate::capture::{AppInfo, AppTarget, CaptureStream};
use crate::config::{AppsSection, AudioDeviceSelector, AudioSection};

const MISSING: &str = "audio capture is not implemented on this platform yet (see src/capture/linux.rs)";

pub fn list_devices() {
    eprintln!("{MISSING}");
}

pub fn list_apps() -> Result<Vec<AppInfo>> {
    bail!(MISSING)
}

pub fn open_device(_sel: &AudioDeviceSelector, _audio: &AudioSection) -> Result<Box<dyn CaptureStream>> {
    bail!(MISSING)
}

pub fn resolve_targets(_apps: &AppsSection) -> Result<Vec<AppTarget>> {
    bail!(MISSING)
}

pub fn open_app(_target: &AppTarget, _audio: &AudioSection) -> Result<Box<dyn CaptureStream>> {
    bail!(MISSING)
}

pub fn process_alive(_pid: u32) -> bool {
    false
}
