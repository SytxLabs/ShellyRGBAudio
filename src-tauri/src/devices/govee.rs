use anyhow::{anyhow, Context, Result};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::net::{SocketAddr, UdpSocket};
use std::sync::{Arc, OnceLock};
use std::time::Duration;

use crate::log_warn;

use super::{shape_brightness, Device, DeviceDescriptor, Frame};

pub const DESCRIPTOR: DeviceDescriptor = DeviceDescriptor {
    type_tag: "govee_lan",
    legacy_key: Some("govees"),
    in_default_config: false,
    default_config: || serde_json::to_value(GoveeLanConfig::default()).expect("serialize govee defaults"),
    build: |entry| {
        let cfg: GoveeLanConfig = serde_json::from_value(entry.clone()).context("parse govee_lan config")?;
        Ok(Box::new(GoveeDevice::new(cfg)?))
    },
};

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct GoveeLanConfig {
    pub ip: String,
    pub name: Option<String>,
    pub min_brightness: u8,
    pub max_brightness: u8,
    pub brightness_gamma: f32,
    pub local_bind_addr: String,
    pub local_port: u16,
    pub remote_port: u16,
    pub read_timeout_ms: u64,
    pub color_temp_kelvin: u32,
    pub dreamview: bool,
}

impl Default for GoveeLanConfig {
    fn default() -> Self {
        Self {
            ip: "192.168.1.60".to_string(),
            name: Some("H6008".to_string()),
            min_brightness: 1,
            max_brightness: 80,
            brightness_gamma: 0.6,
            local_bind_addr: "0.0.0.0".to_string(),
            local_port: 4002,
            remote_port: 4003,
            read_timeout_ms: 300,
            color_temp_kelvin: 0,
            dreamview: false,
        }
    }
}

#[derive(Clone)]
struct SharedSocket {
    lan: Arc<GoveeLan>,
    bind: String,
    port: u16,
    read_timeout_ms: u64,
}
static CLIENT: OnceLock<std::result::Result<SharedSocket, String>> = OnceLock::new();
pub fn shared_client(bind: &str, port: u16, read_timeout_ms: u64) -> Result<Arc<GoveeLan>> {
    let shared = CLIENT
        .get_or_init(|| {
            GoveeLan::new(bind, port, read_timeout_ms).map(|lan| SharedSocket { lan: Arc::new(lan), bind: bind.to_string(), port, read_timeout_ms }).map_err(|e| format!("{e:#}"))
        })
        .clone()
        .map_err(|e| anyhow!(e))?;

    if shared.bind != bind || shared.port != port || shared.read_timeout_ms != read_timeout_ms {
        log_warn!("Warn: govee_lan socket is already bound to {}:{} (read timeout {} ms); ignoring the differing local settings of this entry.", shared.bind, shared.port, shared.read_timeout_ms);
    }
    Ok(shared.lan)
}

pub struct GoveeDevice {
    cfg: GoveeLanConfig,
    lan: Arc<GoveeLan>,
    initial: Option<GoveeState>,
}

impl GoveeDevice {
    pub fn new(cfg: GoveeLanConfig) -> Result<Self> {
        let lan = shared_client(&cfg.local_bind_addr, cfg.local_port, cfg.read_timeout_ms)?;
        let initial = lan.get_state(&cfg.ip, cfg.remote_port).ok();
        if cfg.dreamview {
            lan.set_razer_mode(&cfg.ip, cfg.remote_port, true).ok();
        }
        Ok(Self { cfg, lan, initial })
    }
}

impl Device for GoveeDevice {
    fn name(&self) -> String {
        let dreamview = if self.cfg.dreamview { ", dreamview" } else { "" };
        match &self.cfg.name {
            Some(n) => format!("govee_lan {n} ({}{dreamview})", self.cfg.ip),
            None => format!("govee_lan {}{dreamview}", self.cfg.ip),
        }
    }

    fn apply(&self, frame: &Frame) -> Result<()> {
        let bri = shape_brightness(frame.overall, self.cfg.min_brightness, self.cfg.max_brightness, self.cfg.brightness_gamma);
        let port = self.cfg.remote_port;
        self.lan.turn(&self.cfg.ip, port, true).ok();
        self.lan.set_brightness(&self.cfg.ip, port, bri).ok();
        self.lan.set_rgb(&self.cfg.ip, port, frame.r, frame.g, frame.b, self.cfg.color_temp_kelvin).ok();
        Ok(())
    }

    fn restore(&self) -> Result<()> {
        if self.cfg.dreamview { self.lan.set_razer_mode(&self.cfg.ip, self.cfg.remote_port, false).ok(); }
        if let Some(st) = self.initial { self.lan.restore_state(&self.cfg.ip, self.cfg.remote_port, st, self.cfg.color_temp_kelvin)?; }
        Ok(())
    }
}

pub struct GoveeLan {
    sock: UdpSocket,
}

#[derive(Clone, Copy, Debug)]
pub struct GoveeState {
    pub on: bool,
    pub brightness: u8,
    pub r: u8,
    pub g: u8,
    pub b: u8,
}

impl GoveeLan {
    pub fn new(bind: &str, port: u16, read_timeout_ms: u64) -> Result<Self> {
        let sock = UdpSocket::bind(format!("{bind}:{port}")).with_context(|| format!("bind UDP {bind}:{port} failed"))?;
        sock.set_read_timeout(Some(Duration::from_millis(read_timeout_ms.max(1)))).ok();
        Ok(Self { sock })
    }

    fn send_cmd(&self, ip: &str, port: u16, cmd: &str, data: Value) -> Result<()> {
        let target: SocketAddr = format!("{ip}:{port}").parse().context("bad ip")?;
        let payload = json!({ "msg": { "cmd": cmd, "data": data } });
        let bytes = serde_json::to_vec(&payload).context("serialize govee cmd")?;
        let _ = self.sock.send_to(&bytes, target);
        Ok(())
    }

    pub fn set_rgb(&self, ip: &str, port: u16, r: u8, g: u8, b: u8, color_temp_kelvin: u32) -> Result<()> {
        self.send_cmd(ip, port, "colorwc", json!({ "color": { "r": r, "g": g, "b": b }, "colorTemInKelvin": color_temp_kelvin }))
    }

    pub fn set_brightness(&self, ip: &str, port: u16, brightness_1_100: u8) -> Result<()> {
        let v = brightness_1_100.clamp(1, 100);
        self.send_cmd(ip, port, "brightness", json!({ "value": v }))
    }

    pub fn turn(&self, ip: &str, port: u16, on: bool) -> Result<()> {
        self.send_cmd(ip, port, "turn", json!({ "value": if on { 1 } else { 0 } }))
    }

    pub fn set_razer_mode(&self, ip: &str, port: u16, on: bool) -> Result<()> {
        self.send_pt(ip, port, &razer_mode_packet(on))
    }

    pub fn send_pt(&self, ip: &str, port: u16, packet: &[u8]) -> Result<()> {
        self.send_cmd(ip, port, "razer", json!({ "pt": base64(packet) }))
    }

    pub fn get_state(&self, ip: &str, port: u16) -> Result<GoveeState> {
        self.send_cmd(ip, port, "devStatus", json!({}))?;

        let mut buf = [0u8; 2048];
        let (n, _from) = self.sock.recv_from(&mut buf).context("recv devStatus failed (timeout?)")?;
        let v: Value = serde_json::from_slice(&buf[..n]).context("parse devStatus")?;
        let data = &v["msg"]["data"];

        Ok(GoveeState {
            on: data["onOff"].as_i64().unwrap_or(0) == 1,
            brightness: data["brightness"].as_u64().unwrap_or(100).min(100) as u8,
            r: data["color"]["r"].as_u64().unwrap_or(0).min(255) as u8,
            g: data["color"]["g"].as_u64().unwrap_or(0).min(255) as u8,
            b: data["color"]["b"].as_u64().unwrap_or(0).min(255) as u8,
        })
    }

    pub fn restore_state(&self, ip: &str, port: u16, s: GoveeState, color_temp_kelvin: u32) -> Result<()> {
        self.turn(ip, port, s.on).ok();
        self.set_brightness(ip, port, s.brightness).ok();
        self.set_rgb(ip, port, s.r, s.g, s.b, color_temp_kelvin).ok();
        Ok(())
    }
}

pub fn govee_packet(payload: &[u8]) -> [u8; 20] {
    let mut out = [0u8; 20];
    out[0] = 0x33;
    let n = payload.len().min(18);
    out[1..1 + n].copy_from_slice(&payload[..n]);
    out[19] = out[..19].iter().fold(0u8, |acc, b| acc ^ b);
    out
}

pub fn razer_mode_packet(on: bool) -> [u8; 6] {
    let mut out = [0xBB, 0x00, 0x01, 0xB1, u8::from(on), 0x00];
    out[5] = out[..5].iter().fold(0u8, |acc, b| acc ^ b);
    out
}

pub fn base64(data: &[u8]) -> String {
    const ALPHABET: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
    let mut out = String::with_capacity(data.len().div_ceil(3) * 4);
    for chunk in data.chunks(3) {
        let b = [chunk[0], *chunk.get(1).unwrap_or(&0), *chunk.get(2).unwrap_or(&0)];
        let n = u32::from_be_bytes([0, b[0], b[1], b[2]]);
        for i in 0..4 {
            if i <= chunk.len() {
                out.push(ALPHABET[(n >> (18 - i * 6)) as usize & 0x3F] as char);
            } else {
                out.push('=');
            }
        }
    }
    out
}