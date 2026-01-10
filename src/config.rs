use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};
use std::{fs, path::Path};
use serde_json::Value;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AppConfig {
    pub change_interval_ms: u64,
    pub audio_device: AudioDeviceSelector,
    pub shellys: Vec<ShellyConfig>,
}

impl Default for AppConfig {
    fn default() -> Self {
        Self {
            change_interval_ms: 120,
            audio_device: AudioDeviceSelector::Default,
            shellys: vec![ShellyConfig::default()],
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

fn merge_defaults_into_shellys(merged: &mut Value, defaults: &Value) {
    let def_item = defaults.get("shellys")
        .and_then(|v| v.as_array())
        .and_then(|a| a.first())
        .cloned()
        .unwrap_or_else(|| serde_json::json!({}));

    if let Some(arr) = merged.get_mut("shellys").and_then(|v| v.as_array_mut()) {
        if arr.is_empty() {
            arr.push(def_item);
            return;
        }
        for item in arr.iter_mut() {
            merge_defaults(item, &def_item);
        }
    } else {
        merged["shellys"] = Value::Array(vec![def_item]);
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

    merge_defaults(&mut merged, &defaults_val);
    merge_defaults_into_shellys(&mut merged, &defaults_val);

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