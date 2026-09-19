use anyhow::{Context, Result};
use serde::Serialize;
use serde_json::Value;
use std::sync::{Arc, OnceLock, RwLock};

use crate::color::{BandConfig, ColorEngine, ColorMapConfig, Rgbw};
use crate::config::SpatialSection;
use crate::spatial::{DeviceSpatial, DeviceSpatialConfig, SpeakerLayout};
use crate::log_warn;

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
    fn segment_count(&self) -> usize {
        1
    }
    fn apply_segments(&self, frame: &Frame, _segments: &[Rgbw]) -> Result<()> {
        self.apply(frame)
    }
}

pub struct RuntimeDevice {
    pub inner: Box<dyn Device>,
    pub engine: Option<Arc<ColorEngine>>,
    pub spatial: Option<DeviceSpatial>,
}

impl RuntimeDevice {
    pub fn name(&self) -> String {
        self.inner.name()
    }
    pub fn segment_count(&self) -> usize {
        self.inner.segment_count().max(1)
    }
    pub fn apply(&self, frame: &Frame, segments: &[Rgbw]) -> Result<()> {
        if self.segment_count() > 1 && !segments.is_empty() {
            self.inner.apply_segments(frame, segments)
        } else {
            self.inner.apply(frame)
        }
    }
    pub fn restore(&self) -> Result<()> {
        self.inner.restore()
    }
}

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
        let geometry = DeviceSpatialConfig::default();
        obj.insert("form".to_string(), serde_json::to_value(geometry.form).expect("serialize device form"));
        obj.insert("position".to_string(), Value::Null);
        obj.insert("extent".to_string(), serde_json::to_value(geometry.extent).expect("serialize device extent"));
        obj.insert("path".to_string(), Value::Null);
        obj.insert("spatiality".to_string(), serde_json::to_value(geometry.spatiality).expect("serialize device spatiality"));
    }
    v
}
pub fn default_entries() -> Vec<Value> {
    REGISTRY.iter().filter(|d| d.in_default_config).map(default_entry).collect()
}

#[derive(Debug, Clone, Serialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS), ts(export))]
pub struct DeviceTypeInfo { pub type_tag: String, pub default_entry: Value, }

pub fn type_infos() -> Vec<DeviceTypeInfo> { REGISTRY.iter().map(|d| DeviceTypeInfo { type_tag: d.type_tag.to_string(), default_entry: default_entry(d) }).collect() }

pub fn build_one(entry: &Value) -> Result<Box<dyn Device>> {
    let tag = entry.get("type").and_then(|v| v.as_str()).context("device config entry is missing the \"type\" field")?;
    let desc = find(tag).with_context(|| format!("unknown device type {tag:?}"))?;
    (desc.build)(entry).with_context(|| format!("build device {tag:?}"))
}

pub fn build_all(entries: &[Value], bands: &[BandConfig], global_map: &ColorMapConfig, spatial: &SpatialSection, layout: &SpeakerLayout, warnings: &mut Vec<String>) -> Result<Vec<RuntimeDevice>> {
    let mut devices = Vec::with_capacity(entries.len());
    for entry in entries {
        let tag = entry.get("type").and_then(|v| v.as_str()).context("device config entry is missing the \"type\" field")?;
        let Some(desc) = find(tag) else {
            let known: Vec<&str> = REGISTRY.iter().map(|d| d.type_tag).collect();
            log_warn!("Unknown device type {tag:?} (known: {known:?}). Skipping.");
            continue;
        };
        let inner = (desc.build)(entry).with_context(|| format!("build device {tag:?}"))?;
        let engine = device_engine(entry, bands, global_map, tag, warnings)?;
        let geometry = device_spatial(entry, tag, warnings);
        let points = inner.segment_count().max(1);
        devices.push(RuntimeDevice { inner, engine, spatial: DeviceSpatial::build(&geometry, layout, bands, spatial, points, tag, warnings) });
    }
    Ok(devices)
}

fn device_spatial(entry: &Value, tag: &str, warnings: &mut Vec<String>) -> DeviceSpatialConfig {
    match serde_json::from_value::<DeviceSpatialConfig>(entry.clone()) {
        Ok(cfg) => cfg,
        Err(e) => {
            warnings.push(format!("device {tag:?}: unusable position settings ({e}), it will follow the global mix"));
            DeviceSpatialConfig::default()
        }
    }
}

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

fn overlay(base: &mut Value, patch: &Value) {
    match (base, patch) {
        (Value::Object(b), Value::Object(p)) => {
            for (k, pv) in p {
                match b.get_mut(k) {
                    Some(bv) if bv.is_object() && pv.is_object() => overlay(bv, pv),
                    _ => { b.insert(k.clone(), pv.clone()); }
                }
            }
        }
        (b, p) => *b = p.clone(),
    }
}

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

static LIMITS: OnceLock<RwLock<BrightnessLimits>> = OnceLock::new();
fn limits_slot() -> &'static RwLock<BrightnessLimits> { LIMITS.get_or_init(|| RwLock::new(BrightnessLimits::default())) }
pub fn set_brightness_limits(limits: BrightnessLimits) { *limits_slot().write().unwrap_or_else(|e| e.into_inner()) = limits; }
fn limits() -> BrightnessLimits {
    *limits_slot().read().unwrap_or_else(|e| e.into_inner())
}
pub fn shape_brightness(overall: f32, min_brightness: u8, max_brightness: u8, gamma: f32) -> u8 {
    let l = limits();
    let min = min_brightness.clamp(0, 100) as f32;
    let max = max_brightness.clamp(1, 100) as f32;
    let shaped = overall.clamp(0.0, 1.0).powf(gamma.clamp(l.gamma_min, l.gamma_max));
    ((min + shaped * (max - min)).round() as u8).max(l.floor)
}