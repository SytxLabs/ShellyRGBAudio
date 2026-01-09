use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};
use std::{fs, path::Path};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AppConfig {
    /// How often you push new colors (also a good default for transitions)
    pub change_interval_ms: u64,
    pub shelly: ShellyConfig,
}

impl Default for AppConfig {
    fn default() -> Self {
        Self {
            change_interval_ms: 120,
            shelly: ShellyConfig::default(),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ShellyConfig {
    /// e.g. "192.168.1.50" or "http://192.168.1.50"
    pub host: String,

    /// "auto" tries to detect Gen1 vs Gen2 via `/shelly`
    pub device: ShellyDevice,
    
    pub max_brightness: u8,

    /// Gen2 RGBW component id (usually 0). Used for Shelly Plus RGBW PM in rgbw profile.
    pub rgbw_id: u8,

    /// Optional auth (Gen1: basic; Gen2: digest)
    pub auth: Option<ShellyAuth>,
}

impl Default for ShellyConfig {
    fn default() -> Self {
        Self {
            host: "192.168.178.84".to_string(),
            device: ShellyDevice::Auto,
            max_brightness: 80,
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

pub fn load_or_create(path: &str) -> Result<AppConfig> {
    if !Path::new(path).exists() {
        let cfg = AppConfig::default();
        let text = serde_json::to_string_pretty(&cfg).context("serialize default config")?;
        fs::write(path, text).with_context(|| format!("write default {path}"))?;
        return Ok(cfg);
    }

    let raw = fs::read_to_string(path).with_context(|| format!("read {path}"))?;
    let cfg: AppConfig = serde_json::from_str(&raw).with_context(|| format!("parse {path}"))?;
    Ok(cfg)
}