use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};
use std::{fs, path::Path};
use serde_json::Value;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AppConfig {
    pub change_interval_ms: u64,
    pub audio_device: AudioDeviceSelector,
    pub shelly: ShellyConfig,
}

impl Default for AppConfig {
    fn default() -> Self {
        Self {
            change_interval_ms: 120,
            audio_device: AudioDeviceSelector::Default,
            shelly: ShellyConfig::default(),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ShellyConfig {
    /// e.g. "192.168.1.50" or "http://192.168.1.50"
    pub host: String,

    /// "auto" tries to detect Gen1 vs. Gen2 via `/shelly`
    pub device: ShellyDevice,
    
    pub max_brightness: u8,

    /// Gamma < 1 → hebt leise Stellen an, macht Range "gefühlt" größer.
    /// Gamma > 1 → macht leise Stellen dunkler, Peaks stärker
    pub brightness_gamma: f32,

    /// Gen2 RGBW component id (usually 0). Used for Shelly Plus RGBW PM in rgbw profile.
    pub rgbw_id: u8,

    /// Optional auth (Gen1: basic; Gen2: digest)
    pub auth: Option<ShellyAuth>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum AudioDeviceSelector {
    /// Nimmt das Standard-Output-Gerät (Console Role)
    Default,
    /// Gerät per WASAPI Device Id auswählen (stabil, am besten)
    Id { id: String },
    /// Gerät per Name auswählen (z.B. "Speakers (Realtek...)")
    Name { name: String },
}


impl Default for ShellyConfig {
    fn default() -> Self {
        Self {
            host: "192.168.178.50".to_string(),
            device: ShellyDevice::Auto,
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
        _ => {} // user gewinnt für Nicht-Objects
    }
}

pub fn load_or_create(path: &str) -> Result<AppConfig> {
    let p = Path::new(path);
    let defaults_cfg = AppConfig::default();
    let defaults_val = serde_json::to_value(&defaults_cfg).context("serialize defaults")?;

    if !p.exists() {
        write_pretty(path, &defaults_val)?;
        eprintln!("Created default {path}. Bitte anpassen und neu starten (optional).");
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

    // Wenn user kein Object ist, ersetzen wir durch defaults
    let mut merged = if parsed_user.is_object() {
        parsed_user
    } else {
        eprintln!("Config root is not an object. Replacing with defaults.");
        defaults_val.clone()
    };

    // Defaults auffüllen
    merge_defaults(&mut merged, &defaults_val);

    // Versuchen zu deserialisieren
    let cfg: AppConfig = match serde_json::from_value(merged.clone()) {
        Ok(c) => c,
        Err(e) => {
            eprintln!("Config has invalid values/types: {e}. Replacing with defaults.");
            write_pretty(path, &defaults_val)?;
            return Ok(defaults_cfg);
        }
    };

    // Datei zurückschreiben (damit fehlende Felder sichtbar ergänzt werden)
    write_pretty(path, &merged)?;

    Ok(cfg)
}

fn write_pretty(path: &str, v: &Value) -> Result<()> {
    let text = serde_json::to_string_pretty(v).context("to_string_pretty")?;
    fs::write(path, text).with_context(|| format!("write {path}"))?;
    Ok(())
}