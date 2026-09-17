//! Device abstraction and auto-registration.
//!
//! Every `*.rs` file in this directory (except `mod.rs`) is picked up by `build.rs` and registered as a device type. A new device therefore only
//! needs a new file here that exposes:
//! ```ignore
//! pub const DESCRIPTOR: DeviceDescriptor = DeviceDescriptor { .. };
//! ```
//! No other file has to be touched - not `mod.rs`, not `config.rs`, not `main.rs`.

use anyhow::{Context, Result};
use serde_json::Value;

include!(concat!(env!("OUT_DIR"), "/device_registry.rs"));

/// One RGBW frame that gets pushed to every configured device.
#[derive(Clone, Copy, Debug)]
pub struct Frame {
    pub r: u8,
    pub g: u8,
    pub b: u8,
    pub w: u8,
    /// Normalized loudness in `0.0->1.0`. Each device maps it through its own brightness curve, so devices with different config stay independent.
    pub overall: f32,
    pub transition_ms: u32,
}

pub trait Device: Send + Sync {
    fn name(&self) -> String;
    fn apply(&self, frame: &Frame) -> Result<()>;
    fn restore(&self) -> Result<()>;
}

/// Static description of a device type, used to build devices from config and to migrate legacy config layouts.
pub struct DeviceDescriptor {
    pub type_tag: &'static str, // Value of the `type` field in a `devices[]` entry of `config.json`.
    pub legacy_key: Option<&'static str>, // Top level array this device type used to live in before the unified `devices` array existed (e.g. `"shellys"`). `None` if there is none.
    pub in_default_config: bool, // Whether a freshly created `config.json` should contain one of these.
    pub default_config: fn() -> Value, // Config entry written into a freshly created `config.json`. The `type` field is added automatically.
    pub build: fn(&Value) -> Result<Box<dyn Device>>, // Builds a live device from its `devices[]` config entry.
}

pub fn find(type_tag: &str) -> Option<&'static DeviceDescriptor> {
    REGISTRY.iter().find(|d| d.type_tag == type_tag)
}
pub fn default_entry(desc: &DeviceDescriptor) -> Value {
    let mut v = (desc.default_config)();
    if let Some(obj) = v.as_object_mut() {
        obj.insert("type".to_string(), Value::String(desc.type_tag.to_string()));
    }
    v
}
pub fn default_entries() -> Vec<Value> {
    REGISTRY.iter().filter(|d| d.in_default_config).map(default_entry).collect()
}

pub fn build_all(entries: &[Value]) -> Result<Vec<Box<dyn Device>>> {
    let mut devices = Vec::with_capacity(entries.len());
    for entry in entries {
        let tag = entry.get("type").and_then(|v| v.as_str()).context("device config entry is missing the \"type\" field")?;
        let Some(desc) = find(tag) else {
            let known: Vec<&str> = REGISTRY.iter().map(|d| d.type_tag).collect();
            eprintln!("Unknown device type {tag:?} (known: {known:?}). Skipping.");
            continue;
        };
        devices.push((desc.build)(entry).with_context(|| format!("build device {tag:?}"))?);
    }
    Ok(devices)
}

/// Maps normalized loudness to a device brightness in `1->100`.
pub fn shape_brightness(overall: f32, min_brightness: u8, max_brightness: u8, gamma: f32) -> u8 {
    let min = min_brightness.clamp(0, 100) as f32;
    ((min + (overall.clamp(0.0, 1.0).powf(gamma.clamp(0.1, 5.0))) * ((max_brightness.clamp(1, 100) as f32) - min)).round() as u8).max(1)
}
