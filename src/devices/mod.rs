//! Device abstraction and auto-registration.
//!
//! Every `*.rs` file in this directory (except `mod.rs`) is picked up by `build.rs` and registered as a device type. A new device therefore only
//! needs a new file here that exposes:
//! ```ignore
//! pub const DESCRIPTOR: DeviceDescriptor = DeviceDescriptor { .. };
//! ```
//! No other file has to be touched - not `mod.rs`, not `config.rs`, not `main.rs`.
//!
//! The per-device color override is parsed here rather than in the device files, so a new device inherits it for free.

use anyhow::{Context, Result};
use serde_json::Value;
use std::sync::{Arc, OnceLock};

use crate::color::{BandConfig, ColorEngine, ColorMapConfig};

include!(concat!(env!("OUT_DIR"), "/device_registry.rs"));

#[derive(Clone, Copy, Debug)]
pub struct Frame {
    pub r: u8,
    pub g: u8,
    pub b: u8,
    pub w: u8,
    pub overall: f32, // Normalized loudness in `0.0->1.0`. Each device maps it through its own brightness curve, so devices with different config stay independent.
    pub transition_ms: u32,
}

pub trait Device: Send + Sync {
    fn name(&self) -> String;
    fn apply(&self, frame: &Frame) -> Result<()>;
    fn restore(&self) -> Result<()>;
}

/// A built device plus the color mapping it wants. `engine` is `None` for the common case of following the global mapping.
pub struct RuntimeDevice {
    pub inner: Box<dyn Device>,
    pub engine: Option<Arc<ColorEngine>>,
}

impl RuntimeDevice {
    pub fn name(&self) -> String {
        self.inner.name()
    }
    pub fn apply(&self, frame: &Frame) -> Result<()> {
        self.inner.apply(frame)
    }
    pub fn restore(&self) -> Result<()> {
        self.inner.restore()
    }
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

pub fn build_all(entries: &[Value], bands: &[BandConfig], global_map: &ColorMapConfig, warnings: &mut Vec<String>) -> Result<Vec<RuntimeDevice>> {
    let mut devices = Vec::with_capacity(entries.len());
    for entry in entries {
        let tag = entry.get("type").and_then(|v| v.as_str()).context("device config entry is missing the \"type\" field")?;
        let Some(desc) = find(tag) else {
            let known: Vec<&str> = REGISTRY.iter().map(|d| d.type_tag).collect();
            eprintln!("Unknown device type {tag:?} (known: {known:?}). Skipping.");
            continue;
        };
        let inner = (desc.build)(entry).with_context(|| format!("build device {tag:?}"))?;
        let engine = device_engine(entry, bands, global_map, tag, warnings)?;
        devices.push(RuntimeDevice { inner, engine });
    }
    Ok(devices)
}

/// Builds a color engine for one device, but only if that device actually overrides something.
fn device_engine(entry: &Value, bands: &[BandConfig], global_map: &ColorMapConfig, tag: &str, warnings: &mut Vec<String>) -> Result<Option<Arc<ColorEngine>>> {
    let map_override = entry.get("color_map");
    let band_override = entry.get("bands");
    if map_override.is_none() && band_override.is_none() {
        return Ok(None);
    }

    let map = match map_override {
        Some(part) => {
            let mut v = serde_json::to_value(global_map).context("serialize global color_map")?;
            overlay(&mut v, part);
            serde_json::from_value::<ColorMapConfig>(v).with_context(|| format!("parse color_map override of device {tag:?}"))?
        }
        None => global_map.clone(),
    };

    let bands = match band_override {
        Some(part) => override_bands(bands, part, tag, warnings),
        None => bands.to_vec(),
    };

    Ok(Some(Arc::new(ColorEngine::new(&bands, &map, warnings))))
}

/// Recursive object overlay: every key present in `patch` replaces the one in `base`, nested objects are merged rather than swapped.
fn overlay(base: &mut Value, patch: &Value) {
    match (base, patch) {
        (Value::Object(b), Value::Object(p)) => {
            for (k, pv) in p {
                match b.get_mut(k) {
                    Some(bv) if bv.is_object() && pv.is_object() => overlay(bv, pv),
                    _ => {
                        b.insert(k.clone(), pv.clone());
                    }
                }
            }
        }
        (b, p) => *b = p.clone(),
    }
}

/// A device may recolor or reweight the global bands, but not move their edges: the band energies are computed once, globally, so the index
/// order has to stay the same for everyone.
fn override_bands(global: &[BandConfig], patch: &Value, tag: &str, warnings: &mut Vec<String>) -> Vec<BandConfig> {
    let Some(list) = patch.as_array() else {
        warnings.push(format!("device {tag:?}: \"bands\" must be an array, ignoring it"));
        return global.to_vec();
    };

    let mut out = global.to_vec();
    for (i, item) in list.iter().enumerate() {
        let Some(obj) = item.as_object() else { continue };

        let idx = match obj.get("name").and_then(|v| v.as_str()) {
            Some(name) => match out.iter().position(|b| b.name == name) {
                Some(p) => p,
                None => {
                    warnings.push(format!("device {tag:?}: no global band named {name:?}, ignoring that override"));
                    continue;
                }
            },
            None if i < out.len() => i,
            None => {
                warnings.push(format!("device {tag:?}: band override #{i} has no name and no matching global band, ignoring it"));
                continue;
            }
        };

        for edge in ["from_hz", "to_hz"] {
            if let Some(v) = obj.get(edge).and_then(|v| v.as_f64()) {
                let global_edge = if edge == "from_hz" { out[idx].from_hz } else { out[idx].to_hz };
                if (v as f32 - global_edge).abs() > f32::EPSILON {
                    warnings.push(format!("device {tag:?}: band {:?} cannot move {edge}, the global {global_edge} Hz stays in effect", out[idx].name));
                }
            }
        }

        if let Some(v) = obj.get("weight").and_then(|v| v.as_f64()) {
            out[idx].weight = v as f32;
        }
        if let Some(v) = obj.get("color") {
            match serde_json::from_value(v.clone()) {
                Ok(c) => out[idx].color = c,
                Err(e) => warnings.push(format!("device {tag:?}: band {:?} has an unusable color ({e}), keeping the global one", out[idx].name)),
            }
        }
    }
    out
}

// ---------------------------------------------------------------- brightness

#[derive(Debug, Clone, Copy)]
pub struct BrightnessLimits {
    pub floor: u8,
    pub gamma_min: f32,
    pub gamma_max: f32,
}

impl Default for BrightnessLimits {
    fn default() -> Self {
        Self { floor: 1, gamma_min: 0.1, gamma_max: 5.0 }
    }
}

static LIMITS: OnceLock<BrightnessLimits> = OnceLock::new();

pub fn set_brightness_limits(limits: BrightnessLimits) {
    let _ = LIMITS.set(limits);
}
fn limits() -> BrightnessLimits {
    LIMITS.get().copied().unwrap_or_default()
}
pub fn shape_brightness(overall: f32, min_brightness: u8, max_brightness: u8, gamma: f32) -> u8 {
    let l = limits();
    let min = min_brightness.clamp(0, 100) as f32;
    let max = max_brightness.clamp(1, 100) as f32;
    let shaped = overall.clamp(0.0, 1.0).powf(gamma.clamp(l.gamma_min, l.gamma_max));
    ((min + shaped * (max - min)).round() as u8).max(l.floor)
}