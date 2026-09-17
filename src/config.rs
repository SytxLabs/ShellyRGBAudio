use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::{fs, path::Path};

use crate::devices;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AppConfig {
    pub change_interval_ms: u64,
    pub audio_device: AudioDeviceSelector,
    pub transition_min_ms: u64,
    pub transition_max_ms: u64,
    pub beat_threshold: f32,
    pub strobe_ms: u64,

    /// Raw device entries. Each one is dispatched by its `type` field to the matching device in `src/devices/`, which owns its own config schema.
    pub devices: Vec<Value>,
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

            devices: devices::default_entries(),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum AudioDeviceSelector {
    Default,
    Id { id: String },
    Name { name: String },
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

    migrate_legacy_devices(&mut merged);
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