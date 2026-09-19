use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::{fs, path::{Path, PathBuf}};

use crate::color::{default_bands, BandConfig, ColorMapConfig, Rgbw};
use crate::devices;
use crate::log_error;
use crate::spatial::{SpeakerRole, Vec3};

#[derive(Debug, Clone, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS), ts(export))]
pub struct AppConfig {
    #[serde(default)]
    pub audio: AudioSection,
    #[serde(default = "default_bands")]
    pub bands: Vec<BandConfig>,
    #[serde(default)]
    pub color_map: ColorMapConfig,
    #[serde(default)]
    pub dynamics: DynamicsSection,
    #[serde(default)]
    pub output: OutputSection,
    #[serde(default)]
    pub spatial: SpatialSection,
    #[serde(default = "devices::default_entries")]
    pub devices: Vec<Value>,
}

impl Default for AppConfig {
    fn default() -> Self {
        Self {
            audio: AudioSection::default(),
            bands: default_bands(),
            color_map: ColorMapConfig::default(),
            dynamics: DynamicsSection::default(),
            output: OutputSection::default(),
            spatial: SpatialSection::default(),
            devices: devices::default_entries(),
        }
    }
}

// ---------------------------------------------------------------- audio

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "snake_case")]
#[cfg_attr(feature = "ts", derive(ts_rs::TS), ts(export))]
pub enum WindowKind {
    #[default]
    Hann,
    Hamming,
    Blackman,
    Rectangular,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "snake_case")]
#[cfg_attr(feature = "ts", derive(ts_rs::TS), ts(export))]
pub enum Downmix {
    #[default]
    Average,
    Left,
    Right,
    AllChannels,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS), ts(export))]
pub struct AudioSection {
    #[serde(default)]
    pub device: AudioDeviceSelector,
    #[serde(default)]
    pub apps: AppsSection, // Per-application capture. While disabled, the whole `device` is captured as before.
    #[serde(default = "d_fft_size")]
    pub fft_size: usize,
    #[serde(default = "d_fft_size")]
    pub hop_size: usize,
    #[serde(default)]
    pub window: WindowKind,
    #[serde(default)]
    pub downmix: Downmix,
    #[serde(default = "d_buffer_hns")]
    #[cfg_attr(feature = "ts", ts(type = "number"))]
    pub buffer_duration_hns: i64, // WASAPI capture buffer in 100 ns units. 200,000 is 20 ms.
    #[serde(default = "d_silence_timeout")]
    #[cfg_attr(feature = "ts", ts(type = "number"))]
    pub silence_timeout_ms: u64, // No audio for this long counts as silence.
    #[serde(default = "d_silence_fade")]
    pub silence_fade_ms: u32, // Fade time used when dimming down into silence.
    #[serde(default)]
    pub silence_brightness: f32, // Brightness held during silence, `0.0` to `1.0`.
}

fn d_fft_size() -> usize { 1024 }
fn d_buffer_hns() -> i64 { 200_000 }
fn d_silence_timeout() -> u64 { 2000 }
fn d_silence_fade() -> u32 { 800 }

impl Default for AudioSection {
    fn default() -> Self {
        Self {
            device: AudioDeviceSelector::Default,
            apps: AppsSection::default(),
            fft_size: d_fft_size(),
            hop_size: d_fft_size(),
            window: WindowKind::default(),
            downmix: Downmix::default(),
            buffer_duration_hns: d_buffer_hns(),
            silence_timeout_ms: d_silence_timeout(),
            silence_fade_ms: d_silence_fade(),
            silence_brightness: 0.0,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
#[serde(tag = "type", rename_all = "snake_case")]
#[cfg_attr(feature = "ts", derive(ts_rs::TS), ts(export))]
pub enum AudioDeviceSelector {
    #[default]
    Default,
    Id { id: String },
    Name { name: String },
}

// ---------------------------------------------------------------- audio.apps
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "snake_case")]
#[cfg_attr(feature = "ts", derive(ts_rs::TS), ts(export))]
pub enum AppMatchMode {
    #[default]
    Include,
    Exclude,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS), ts(export))]
pub struct AppsSection {
    #[serde(default)]
    pub enabled: bool, // `false` captures the output device, exactly like before.
    #[serde(default)]
    pub mode: AppMatchMode,
    #[serde(default = "d_true")]
    pub include_process_tree: bool, // Also capture a child process of a matched app, even when its executable has another name. Browsers play through a child process, so this keeps working when that child is named differently. Windows always captures the process tree, so the flag has no effect there.
    #[serde(default = "d_rescan")]
    #[cfg_attr(feature = "ts", ts(type = "number"))]
    pub rescan_ms: u64, // How often the running applications are rechecked, so an app that starts, restarts or plays its first sound later is picked up.
    #[serde(default)]
    #[cfg_attr(feature = "ts", ts(type = "number | null"))]
    pub sample_rate: Option<u32>, // Capture format. Per-app capture has no mix format of its own to ask for, so `null` takes the output device's.
    #[serde(default)]
    #[cfg_attr(feature = "ts", ts(type = "number | null"))]
    pub channels: Option<u16>, // `null` follows the output device too, which is what keeps a 7.1 setup from being recorded as stereo.
    #[serde(default)]
    pub groups: Vec<AppGroup>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS), ts(export))]
pub struct AppGroup {
    #[serde(default)]
    pub name: String,
    #[serde(default = "d_true")]
    pub enabled: bool,
    #[serde(default)]
    pub apps: Vec<String>, // Matched case insensitively against the display name ("Google Chrome") and the executable ("chrome.exe").
    #[serde(default = "d_one")]
    pub gain: f32,
}

fn d_true() -> bool { true }
fn d_rescan() -> u64 { 3000 }

impl Default for AppsSection {
    fn default() -> Self {
        Self {
            enabled: false,
            mode: AppMatchMode::default(),
            include_process_tree: d_true(),
            rescan_ms: d_rescan(),
            sample_rate: None,
            channels: None,
            groups: Vec::new(),
        }
    }
}

impl AppsSection {
    pub fn active_groups(&self) -> impl Iterator<Item = &AppGroup> {
        self.groups.iter().filter(|g| g.enabled && g.apps.iter().any(|p| !p.trim().is_empty()))
    }

    pub fn is_usable(&self, warnings: &mut Vec<String>) -> bool {
        if !self.enabled {
            return false;
        }
        if self.active_groups().next().is_none() {
            warnings.push("audio.apps.enabled is true but no enabled group names an app, capturing the output device instead".to_string());
            return false;
        }
        true
    }
}

// ---------------------------------------------------------------- dynamics

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "snake_case")]
#[cfg_attr(feature = "ts", derive(ts_rs::TS), ts(export))]
pub enum Normalize {
    #[default]
    Shared,
    PerBand,
}

/// Where the brightness level comes from.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "snake_case")]
#[cfg_attr(feature = "ts", derive(ts_rs::TS), ts(export))]
pub enum LevelSource {
    #[default]
    Peak,
    Average,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS), ts(export))]
pub struct DynamicsSection {
    #[serde(default)]
    pub normalize: Normalize,
    #[serde(default)]
    pub level_source: LevelSource,
    
    #[serde(default = "d_one")]
    pub log_offset: f32, // Offset in the `ln(x + offset)` loudness compression. Larger values flatten quiet passages.
    #[serde(default = "d_peak_floor")]
    pub peak_floor: f32, // Lower bound of the per-band peak tracker. Also acts as the noise floor.
    #[serde(default = "d_peak_decay")]
    pub peak_decay: f32, // Per-frame decay of the per-band peak tracker. Closer to 1 adapts more slowly.
    #[serde(default = "d_level_alpha")]
    pub level_alpha: f32, // Smoothing of the overall level. Smaller reacts less, larger reacts faster.
    #[serde(default = "d_band_attack")]
    pub band_attack: f32, // How fast a band's energy is allowed to *rise*. This is what decides how quickly the color answers the music, so keep it high. `1.0` follows every frame instantly.
    
    #[serde(default = "d_band_release")]
    pub band_release: f32, // How fast a band's energy is allowed to *fall*. Lower values make the color glide instead of flickering, without costing any responsibility, because rises are governed by `band_attack`. Beat detection uses the unsmoothed values either way, so neither knob dulls the strobe.
    #[serde(default = "d_flux_alpha")]
    pub flux_alpha: f32, // Smoothing of the spectral flux used for beat detection.
    #[serde(default = "d_beat_threshold")]
    pub beat_threshold: f32,
    #[serde(default)]
    #[cfg_attr(feature = "ts", ts(type = "number"))]
    pub beat_cooldown_ms: u64, // Minimum time between two beats. `0` keeps the old behavior where every frame above the threshold retriggers.
    
    #[serde(default = "d_strobe_ms")]
    #[cfg_attr(feature = "ts", ts(type = "number"))]
    pub strobe_ms: u64, // How long a beat holds the strobe.
    #[serde(default = "d_one")]
    pub strobe_level: f32, // Brightness during a strobe, `0.0` to `1.0`.
    #[serde(default)]
    #[cfg_attr(feature = "ts", ts(type = "string | null"))]
    pub strobe_color: Option<Rgbw>, // Color forced during a strobe. `null` keeps the music color.
}

fn d_one() -> f32 { 1.0 }
fn d_peak_floor() -> f32 { 1e-6 }
fn d_peak_decay() -> f32 { 0.995 }
fn d_level_alpha() -> f32 { 0.12 }
pub(crate) fn d_band_attack() -> f32 { 0.45 }
pub(crate) fn d_band_release() -> f32 { 0.12 }
fn d_flux_alpha() -> f32 { 0.25 }
fn d_beat_threshold() -> f32 { 0.18 }
fn d_strobe_ms() -> u64 { 40 }

impl Default for DynamicsSection {
    fn default() -> Self {
        Self {
            normalize: Normalize::default(),
            level_source: LevelSource::default(),
            log_offset: d_one(),
            peak_floor: d_peak_floor(),
            peak_decay: d_peak_decay(),
            level_alpha: d_level_alpha(),
            band_attack: d_band_attack(),
            band_release: d_band_release(),
            flux_alpha: d_flux_alpha(),
            beat_threshold: d_beat_threshold(),
            beat_cooldown_ms: 0,
            strobe_ms: d_strobe_ms(),
            strobe_level: d_one(),
            strobe_color: None,
        }
    }
}

// ---------------------------------------------------------------- output

#[derive(Debug, Clone, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS), ts(export))]
pub struct OutputSection {
    #[serde(default = "d_change_interval")]
    #[cfg_attr(feature = "ts", ts(type = "number"))]
    pub change_interval_ms: u64, // Minimum time between two commands sent to the lights. Lower reacts faster but stresses the device.
    #[serde(default = "d_transition_min")]
    #[cfg_attr(feature = "ts", ts(type = "number"))]
    pub transition_min_ms: u64,
    #[serde(default = "d_transition_max")]
    #[cfg_attr(feature = "ts", ts(type = "number"))]
    pub transition_max_ms: u64,
    #[serde(default = "d_beat_weight")]
    pub transition_beat_weight: f32, // How much beat strength shortens the transition.
    #[serde(default = "d_level_weight")]
    pub transition_level_weight: f32, // How much the overall level shortens the transition.
    #[serde(default = "d_one")]
    pub transition_curve: f32, // Shapes the transition ramp. `1.0` is linear, `> 1` keeps transitions long until the music really picks up.
    #[serde(default = "d_deadband_rgb")]
    pub deadband_rgb: u8, // A frame is only sent if a channel moved at least this much.
    #[serde(default = "d_deadband_overall")]
    pub deadband_overall: f32, // ... or the overall level moved at least this much.
    #[serde(default = "d_brightness_floor")]
    pub brightness_floor: u8, // Lowest brightness ever sent to a light. `1` keeps lights from switching off completely.
    #[serde(default = "d_gamma_min")]
    pub gamma_min: f32,
    #[serde(default = "d_gamma_max")]
    pub gamma_max: f32,
}

fn d_change_interval() -> u64 { 120 }
fn d_transition_min() -> u64 { 60 }
fn d_transition_max() -> u64 { 600 }
fn d_beat_weight() -> f32 { 0.75 }
fn d_level_weight() -> f32 { 0.25 }
fn d_deadband_rgb() -> u8 { 3 }
fn d_deadband_overall() -> f32 { 0.02 }
fn d_brightness_floor() -> u8 { 1 }
fn d_gamma_min() -> f32 { 0.1 }
fn d_gamma_max() -> f32 { 5.0 }

impl Default for OutputSection {
    fn default() -> Self {
        Self {
            change_interval_ms: d_change_interval(),
            transition_min_ms: d_transition_min(),
            transition_max_ms: d_transition_max(),
            transition_beat_weight: d_beat_weight(),
            transition_level_weight: d_level_weight(),
            transition_curve: d_one(),
            deadband_rgb: d_deadband_rgb(),
            deadband_overall: d_deadband_overall(),
            brightness_floor: d_brightness_floor(),
            gamma_min: d_gamma_min(),
            gamma_max: d_gamma_max(),
        }
    }
}

// ---------------------------------------------------------------- spatial

/// Which speaker sits on which capture channel.
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
#[serde(rename_all = "snake_case")]
#[cfg_attr(feature = "ts", derive(ts_rs::TS), ts(export))]
pub enum LayoutSelector {
    #[default]
    Auto, // Decoded from the channel mask the audio backend reports.
    Channels(Vec<SpeakerRole>), // Stated by hand, one entry per capture channel, in interleaved order.
}

/// `#[serde(default)]` so stating only one corner keeps the other at its default rather than making the whole config unreadable.
#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
#[serde(default)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS), ts(export))]
pub struct RoomBounds {
    #[cfg_attr(feature = "ts", ts(type = "[number, number, number]"))]
    pub min: Vec3,
    #[cfg_attr(feature = "ts", ts(type = "[number, number, number]"))]
    pub max: Vec3,
}

impl Default for RoomBounds {
    fn default() -> Self {
        Self { min: Vec3::new(-3.0, 0.0, -3.0), max: Vec3::new(3.0, 3.0, 3.0) }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS), ts(export))]
pub struct SpatialSection {
    #[serde(default)]
    pub enabled: bool,
    #[serde(default)]
    pub layout: LayoutSelector,
    #[serde(default)]
    pub room: RoomBounds, // Normalizes a device's height into the band spectrum. Only `min.y`/`max.y` are read today.
    #[serde(default = "d_focus")]
    pub focus: f32, // Exponent on the direction match. Higher values aim a light more tightly at the speakers it faces.
    #[serde(default = "d_omni_floor")]
    pub omni_floor: f32, // Share of the audio every device hears regardless of direction, so a hard left light never loses the right channel completely.
    #[serde(default = "d_strip_samples")]
    pub strip_samples: usize, // How many points a `strip` device is sampled at along its segment.
    #[serde(default = "d_height_sharpness")]
    pub height_sharpness: f32, // Width of the height-to-frequency match. Smaller values tie a height to fewer bands.
    #[serde(default)]
    pub distance_falloff: f32, // Brightness lost per metre of distance from the listener. `0.0` keeps every light equally bright.
}

fn d_focus() -> f32 { 2.0 }
fn d_omni_floor() -> f32 { 0.15 }
fn d_strip_samples() -> usize { 8 }
fn d_height_sharpness() -> f32 { 0.35 }

impl Default for SpatialSection {
    fn default() -> Self {
        Self {
            enabled: false,
            layout: LayoutSelector::default(),
            room: RoomBounds::default(),
            focus: d_focus(),
            omni_floor: d_omni_floor(),
            strip_samples: d_strip_samples(),
            height_sharpness: d_height_sharpness(),
            distance_falloff: 0.0,
        }
    }
}

// ---------------------------------------------------------------- loading

fn merge_defaults(user: &mut Value, defaults: &Value) {
    if let (Value::Object(u), Value::Object(d)) = (user, defaults) {
        for (k, dv) in d {
            match u.get_mut(k) {
                Some(uv) => merge_defaults(uv, dv),
                None => {
                    u.insert(k.clone(), dv.clone());
                }
            }
        }
    }
}

fn migrate_legacy_devices(merged: &mut Value) {
    if merged.get("shellys").is_none() && let Some(old) = merged.get("shelly").cloned()
    {
        merged.as_object_mut().unwrap().remove("shelly");
        merged["shellys"] = Value::Array(vec![old]);
    }

    if merged.get("devices").is_none() {
        let mut list = Vec::new();

        for desc in devices::REGISTRY {
            let Some(key) = desc.legacy_key else { continue };
            let Some(arr) = merged.get(key).and_then(|v| v.as_array()) else { continue };

            for it in arr {
                let mut entry = serde_json::json!({ "type": desc.type_tag });
                if let (Some(obj), Some(src)) = (entry.as_object_mut(), it.as_object()) {
                    for (k, v) in src {
                        obj.insert(k.clone(), v.clone());
                    }
                }
                list.push(entry);
            }
        }

        if !list.is_empty() {
            merged["devices"] = Value::Array(list);
        }
    }

    if let Some(obj) = merged.as_object_mut() {
        for desc in devices::REGISTRY {
            if let Some(key) = desc.legacy_key {
                obj.remove(key);
            }
        }
    }
}

fn migrate_legacy_top_level(merged: &mut Value) {
    const MOVES: &[(&str, &str, &str)] = &[
        ("audio_device", "audio", "device"),
        ("change_interval_ms", "output", "change_interval_ms"),
        ("transition_min_ms", "output", "transition_min_ms"),
        ("transition_max_ms", "output", "transition_max_ms"),
        ("beat_threshold", "dynamics", "beat_threshold"),
        ("strobe_ms", "dynamics", "strobe_ms"),
    ];

    for (old_key, section, new_key) in MOVES {
        let Some(obj) = merged.as_object_mut() else { return };
        let Some(val) = obj.remove(*old_key) else { continue };

        let entry = obj.entry((*section).to_string()).or_insert_with(|| Value::Object(Default::default()));
        if let Some(sec) = entry.as_object_mut() && !sec.contains_key(*new_key)
        {
            sec.insert((*new_key).to_string(), val);
        }
    }
    if let Some(dyn_sec) = merged.get_mut("dynamics").and_then(|v| v.as_object_mut()) && let Some(alpha) = dyn_sec.remove("band_alpha")
    {
        for key in ["band_attack", "band_release"] {
            if !dyn_sec.contains_key(key) {
                dyn_sec.insert(key.to_string(), alpha.clone());
            }
        }
    }

    if let Some(map) = merged.get_mut("color_map").and_then(|v| v.as_object_mut()) && let Some(Value::Bool(on)) = map.remove("saturate") && !map.contains_key("saturation")
    {
        map.insert("saturation".to_string(), serde_json::json!(if on { 1.0 } else { 0.0 }));
    }
}

/// What [`load`] found on disk. The config itself is always usable; the variant says how much of it came from the file.
pub enum LoadOutcome {
    /// The file was read and, where needed, migrated forward.
    Loaded { cfg: AppConfig, warnings: Vec<String> },
    /// Nothing was there. `cfg` holds the built-in defaults and no file was written.
    Missing { cfg: AppConfig },
    /// The file could not be used. A copy was put at `backup` and `cfg` holds the built-in defaults.
    ///
    /// The original is deliberately left alone: this used to overwrite it with defaults on the spot, which destroyed a config over a single mistyped
    /// value before anyone could be told about it. Saving from the UI is what replaces it now.
    Recovered { cfg: AppConfig, backup: Option<PathBuf>, error: String },
}

impl LoadOutcome {
    pub fn config(&self) -> &AppConfig {
        match self {
            Self::Loaded { cfg, .. } | Self::Missing { cfg } | Self::Recovered { cfg, .. } => cfg,
        }
    }
    pub fn into_config(self) -> AppConfig {
        match self {
            Self::Loaded { cfg, .. } | Self::Missing { cfg } | Self::Recovered { cfg, .. } => cfg,
        }
    }
}

/// Reads the config, migrating legacy layouts and filling in defaults. Never writes to `path`.
pub fn load(path: &Path) -> Result<LoadOutcome> {
    let defaults_cfg = AppConfig::default();
    let defaults_val = serde_json::to_value(&defaults_cfg).context("serialize defaults")?;

    if !path.exists() {
        return Ok(LoadOutcome::Missing { cfg: defaults_cfg });
    }

    let raw = fs::read_to_string(path).with_context(|| format!("read {}", path.display()))?;
    let parsed_user: Value = match serde_json::from_str(raw.trim_start_matches('\u{feff}')) {
        Ok(v) => v,
        Err(e) => return Ok(recover(path, &raw, defaults_cfg, format!("the file is not valid JSON: {e}"))),
    };
    if !parsed_user.is_object() {
        return Ok(recover(path, &raw, defaults_cfg, "the top level of the file is not a JSON object".to_string()));
    }

    let mut merged = parsed_user;
    migrate_legacy_top_level(&mut merged);
    migrate_legacy_devices(&mut merged);
    merge_defaults(&mut merged, &defaults_val);

    match serde_json::from_value::<AppConfig>(merged) {
        Ok(cfg) => Ok(LoadOutcome::Loaded { cfg, warnings: Vec::new() }),
        Err(e) => Ok(recover(path, &raw, defaults_cfg, format!("a setting has an unusable value or type: {e}"))),
    }
}

fn recover(path: &Path, previous: &str, defaults: AppConfig, error: String) -> LoadOutcome {
    let target = path.with_extension("json.bak");
    let backup = match fs::write(&target, previous) {
        Ok(()) => Some(target),
        Err(e) => {
            log_error!("Could not write {} (ignored): {e}", target.display());
            None
        }
    };
    LoadOutcome::Recovered { cfg: defaults, backup, error }
}

/// Writes the config. The new file is put beside the target and renamed over it, so an interrupted write leaves the previous one intact rather than a
/// half-written file that the next start would then reject.
pub fn save(path: &Path, cfg: &AppConfig) -> Result<()> {
    let text = serde_json::to_string_pretty(cfg).context("serialize the config")?;

    if let Some(dir) = path.parent().filter(|d| !d.as_os_str().is_empty()) {
        fs::create_dir_all(dir).with_context(|| format!("create {}", dir.display()))?;
    }
    let tmp = path.with_extension("json.tmp");
    fs::write(&tmp, text).with_context(|| format!("write {}", tmp.display()))?;
    fs::rename(&tmp, path).with_context(|| format!("replace {}", path.display()))?;
    Ok(())
}