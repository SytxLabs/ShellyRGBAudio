use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};
use std::{fs, path::Path};
use serde_json::Value;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AppConfig {
    pub change_interval_ms: u64,
    pub audio_device: AudioDeviceSelector,
    pub transition_min_ms: u64,
    pub transition_max_ms: u64,
    pub beat_threshold: f32,
    pub strobe_ms: u64,

    pub devices: Vec<DeviceConfig>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum DeviceConfig {
    Shelly(ShellyConfig),
    GoveeLan(GoveeLanConfig),
}

impl Default for AppConfig {
    fn default() -> Self {
        Self {
            change_interval_ms: 120,
            audio_device: AudioDeviceSelector::Default,
            transition_min_ms: 60,
            transition_max_ms: 600,
            beat_threshold: 0.18,
            strobe_ms: 40,

            devices: vec![DeviceConfig::Shelly(ShellyConfig::default())],
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ShellyConfig {
    pub host: String,
    pub device: ShellyDevice,
    pub min_brightness: u8,
    pub max_brightness: u8,
    pub brightness_gamma: f32,
    pub rgbw_id: u8,
    pub auth: Option<ShellyAuth>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GoveeLanConfig {
    pub ip: String,
    pub name: Option<String>,
    pub min_brightness: u8,
    pub max_brightness: u8,
    pub brightness_gamma: f32,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum AudioDeviceSelector {
    Default,
    Id { id: String },
    Name { name: String },
}


impl Default for ShellyConfig {
    fn default() -> Self {
        Self {
            host: "192.168.178.50".to_string(),
            device: ShellyDevice::Auto,
            min_brightness: 1,
            max_brightness: 80,
            brightness_gamma: 0.6,
            rgbw_id: 0,
            auth: None,
        }
    }
}

impl Default for GoveeLanConfig {
    fn default() -> Self {
        Self {
            ip: "192.168.1.60".to_string(),
            name: Some("H6008".to_string()),
            min_brightness: 1,
            max_brightness: 80,
            brightness_gamma: 0.6,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ShellyAuth {
    pub username: String,
    pub password: String,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ShellyDevice {
    Auto,
    Rgbw2,        // Gen1
    PlusRgbwPm,   // Gen2
}

fn merge_defaults(user: &mut Value, defaults: &Value) {
    match (user, defaults) {
        (Value::Object(u), Value::Object(d)) => {
            for (k, dv) in d {
                match u.get_mut(k) {
                    Some(uv) => merge_defaults(uv, dv),
                    None => {
                        u.insert(k.clone(), dv.clone());
                    }
                }
            }
        }
        _ => {}
    }
}


pub fn load_or_create(path: &str) -> Result<AppConfig> {
    let p = Path::new(path);
    let defaults_cfg = AppConfig::default();
    let defaults_val = serde_json::to_value(&defaults_cfg).context("serialize defaults")?;

    if !p.exists() {
        write_pretty(path, &defaults_val)?;
        eprintln!("Created default {path}. Please adjust and restart (optional).");
        return Ok(defaults_cfg);
    }

    let raw = fs::read_to_string(p).with_context(|| format!("read {path}"))?;
    let parsed_user: Value = match serde_json::from_str(&raw) {
        Ok(v) => v,
        Err(e) => {
            eprintln!("Config JSON invalid ({path}): {e}. Replacing with defaults.");
            write_pretty(path, &defaults_val)?;
            return Ok(defaults_cfg);
        }
    };

    let mut merged = if parsed_user.is_object() {
        parsed_user
    } else {
        eprintln!("Config root is not an object. Replacing with defaults.");
        defaults_val.clone()
    };
    if merged.get("shellys").is_none() {
        if let Some(old) = merged.get("shelly").cloned() {
            merged.as_object_mut().unwrap().remove("shelly");
            merged["shellys"] = Value::Array(vec![old]);
        }
    }

    if merged.get("devices").is_none() {
        let mut devices = Vec::new();

        if let Some(arr) = merged.get("shellys").and_then(|v| v.as_array()) {
            for it in arr {
                devices.push(serde_json::json!({"type": "shelly"}));
                if let Some(obj) = devices.last_mut().and_then(|v| v.as_object_mut()) {
                    if let Some(src) = it.as_object() {
                        for (k, v) in src {
                            obj.insert(k.clone(), v.clone());
                        }
                    }
                }
            }
        }

        if let Some(arr) = merged.get("govees").and_then(|v| v.as_array()) {
            for it in arr {
                devices.push(serde_json::json!({ "type": "govee_lan" }));
                if let Some(obj) = devices.last_mut().and_then(|v| v.as_object_mut()) {
                    if let Some(src) = it.as_object() {
                        for (k, v) in src {
                            obj.insert(k.clone(), v.clone());
                        }
                    }
                }
            }
        }

        if !devices.is_empty() {
            merged["devices"] = Value::Array(devices);
        }
    }
    if let Some(obj) = merged.as_object_mut() {
        obj.remove("shellys");
        obj.remove("govees");
    }

    merge_defaults(&mut merged, &defaults_val);

    let cfg: AppConfig = match serde_json::from_value(merged.clone()) {
        Ok(c) => c,
        Err(e) => {
            eprintln!("Config has invalid values/types: {e}. Replacing with defaults.");
            write_pretty(path, &defaults_val)?;
            return Ok(defaults_cfg);
        }
    };
    write_pretty(path, &merged)?;
    Ok(cfg)
}

fn write_pretty(path: &str, v: &Value) -> Result<()> {
    let text = serde_json::to_string_pretty(v).context("to_string_pretty")?;
    fs::write(path, text).with_context(|| format!("write {path}"))?;
    Ok(())
}