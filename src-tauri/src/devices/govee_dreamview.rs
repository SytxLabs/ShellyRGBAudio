use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};
use std::sync::Arc;

use crate::{log_info, log_warn};

use super::govee::{govee_packet, GoveeLan, GoveeState};
use super::{shape_brightness, Device, DeviceDescriptor, Frame};
use crate::color::Rgbw;

pub const DESCRIPTOR: DeviceDescriptor = DeviceDescriptor {
    type_tag: "govee_dreamview",
    legacy_key: None,
    in_default_config: false,
    default_config: || serde_json::to_value(DreamViewConfig::default()).expect("serialize govee_dreamview defaults"),
    build: |entry| {
        let cfg: DreamViewConfig = serde_json::from_value(entry.clone()).context("parse govee_dreamview config")?;
        Ok(Box::new(DreamViewDevice::new(cfg)?))
    },
};

const MAX_SEGMENTS: usize = 15;

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct DreamViewConfig {
    pub ip: String,
    pub name: Option<String>,
    pub segments: usize,
    pub reverse: bool, // Flips which end of the strip segment 0 sits at.
    pub min_brightness: u8,
    pub max_brightness: u8,
    pub brightness_gamma: f32,
    pub local_bind_addr: String,
    pub local_port: u16,
    pub remote_port: u16,
    pub read_timeout_ms: u64,
    pub color_temp_kelvin: u32,
}

impl Default for DreamViewConfig {
    fn default() -> Self {
        Self {
            ip: "192.168.1.61".to_string(),
            name: Some("DreamView".to_string()),
            segments: 1,
            reverse: false,
            min_brightness: 1,
            max_brightness: 80,
            brightness_gamma: 0.6,
            local_bind_addr: "0.0.0.0".to_string(),
            local_port: 4002,
            remote_port: 4003,
            read_timeout_ms: 300,
            color_temp_kelvin: 0,
        }
    }
}

pub struct DreamViewDevice {
    cfg: DreamViewConfig,
    lan: Arc<GoveeLan>,
    segments: usize,
    initial: Option<GoveeState>,
}

impl DreamViewDevice {
    pub fn new(mut cfg: DreamViewConfig) -> Result<Self> {
        let asked = cfg.segments;
        cfg.segments = cfg.segments.clamp(1, MAX_SEGMENTS);
        if asked > MAX_SEGMENTS {
            log_warn!("Warn: govee_dreamview {}: Govee's segment bitmap holds at most {MAX_SEGMENTS} segments, using that instead of {asked}.", cfg.ip);
        }

        let lan = super::govee::shared_client(&cfg.local_bind_addr, cfg.local_port, cfg.read_timeout_ms)?;
        let initial = lan.get_state(&cfg.ip, cfg.remote_port).ok();
        lan.set_razer_mode(&cfg.ip, cfg.remote_port, true).ok();

        if cfg.segments > 1 {
            log_info!("Note: govee_dreamview {} is set to {} segments. Segment addressing over LAN is not part of Govee's published API and may be ignored by your model; set \"segments\": 1 if the strip does not follow.", cfg.ip, cfg.segments);
        }
        let segments = cfg.segments;
        Ok(Self { cfg, lan, segments, initial })
    }

    fn brightness(&self, frame: &Frame) -> u8 {
        shape_brightness(frame.overall, self.cfg.min_brightness, self.cfg.max_brightness, self.cfg.brightness_gamma)
    }
}

impl Device for DreamViewDevice {
    fn name(&self) -> String {
        let label = self.cfg.name.clone().unwrap_or_else(|| "govee_dreamview".to_string());
        format!("govee_dreamview {label} ({}, {} segments)", self.cfg.ip, self.segments)
    }

    fn apply(&self, frame: &Frame) -> Result<()> {
        let port = self.cfg.remote_port;
        self.lan.turn(&self.cfg.ip, port, true).ok();
        self.lan.set_brightness(&self.cfg.ip, port, self.brightness(frame)).ok();
        self.lan.set_rgb(&self.cfg.ip, port, frame.r, frame.g, frame.b, self.cfg.color_temp_kelvin).ok();
        Ok(())
    }

    fn restore(&self) -> Result<()> {
        self.lan.set_razer_mode(&self.cfg.ip, self.cfg.remote_port, false).ok();
        if let Some(st) = self.initial {
            self.lan.restore_state(&self.cfg.ip, self.cfg.remote_port, st, self.cfg.color_temp_kelvin)?;
        }
        Ok(())
    }

    fn segment_count(&self) -> usize { self.segments }

    fn apply_segments(&self, frame: &Frame, segments: &[Rgbw]) -> Result<()> {
        let port = self.cfg.remote_port;
        self.lan.turn(&self.cfg.ip, port, true).ok();
        self.lan.set_brightness(&self.cfg.ip, port, self.brightness(frame)).ok();
        for packet in segment_packets(segments, self.segments, self.cfg.reverse) {
            self.lan.send_pt(&self.cfg.ip, port, &packet).ok();
        }
        Ok(())
    }
}

fn segment_packets(colors: &[Rgbw], segments: usize, reverse: bool) -> Vec<[u8; 20]> {
    let segments = segments.clamp(1, MAX_SEGMENTS);
    if colors.is_empty() {
        return Vec::new();
    }
    let mut groups: Vec<(Rgbw, u16)> = Vec::new();
    for seg in 0..segments {
        let c = colors[(seg * colors.len() / segments).min(colors.len() - 1)];
        let bit = 1u16 << if reverse { segments - 1 - seg } else { seg };
        match groups.iter_mut().find(|(gc, _)| *gc == c) {
            Some((_, mask)) => *mask |= bit,
            None => groups.push((c, bit)),
        }
    }
    groups.into_iter().map(|(c, mask)| {
        let [lo, hi] = mask.to_le_bytes();
        govee_packet(&[0x05, 0x15, 0x01, c.r, c.g, c.b, 0, 0, 0, 0, 0, 0, 0, lo, hi])
    }).collect()
}