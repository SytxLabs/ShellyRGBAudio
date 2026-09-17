use anyhow::{anyhow, Context, Result};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::net::{SocketAddr, UdpSocket};
use std::sync::{Arc, OnceLock};
use std::time::Duration;

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
pub struct GoveeLanConfig {
    pub ip: String,
    pub name: Option<String>,
    pub min_brightness: u8,
    pub max_brightness: u8,
    pub brightness_gamma: f32,
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

/// All Govee LAN lights share one UDP socket, because the protocol requires the local port 4002, and it can only be bound once per process.
static CLIENT: OnceLock<std::result::Result<Arc<GoveeLan>, String>> = OnceLock::new();

fn client() -> Result<Arc<GoveeLan>> {
    CLIENT.get_or_init(|| GoveeLan::new().map(Arc::new).map_err(|e| format!("{e:#}"))).clone().map_err(|e| anyhow!(e))
}

pub struct GoveeDevice {
    cfg: GoveeLanConfig,
    lan: Arc<GoveeLan>,
    initial: Option<GoveeState>,
}

impl GoveeDevice {
    pub fn new(cfg: GoveeLanConfig) -> Result<Self> {
        let lan = client()?;
        let initial = lan.get_state(&cfg.ip).ok();
        Ok(Self { cfg, lan, initial })
    }
}

impl Device for GoveeDevice {
    fn name(&self) -> String {
        match &self.cfg.name {
            Some(n) => format!("govee_lan {n} ({})", self.cfg.ip),
            None => format!("govee_lan {}", self.cfg.ip),
        }
    }

    fn apply(&self, frame: &Frame) -> Result<()> {
        let bri = shape_brightness(frame.overall, self.cfg.min_brightness, self.cfg.max_brightness, self.cfg.brightness_gamma);
        self.lan.turn(&self.cfg.ip, true).ok();
        self.lan.set_brightness(&self.cfg.ip, bri).ok();
        self.lan.set_rgb(&self.cfg.ip, frame.r, frame.g, frame.b).ok();
        Ok(())
    }

    fn restore(&self) -> Result<()> {
        if let Some(st) = self.initial {
            self.lan.restore_state(&self.cfg.ip, st)?;
        }
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
    pub fn new() -> Result<Self> {
        let sock = UdpSocket::bind("0.0.0.0:4002").context("bind UDP 4002 failed")?;
        sock.set_read_timeout(Some(Duration::from_millis(300))).ok();
        Ok(Self { sock })
    }

    fn send_cmd(&self, ip: &str, cmd: &str, data: Value) -> Result<()> {
        let target: SocketAddr = format!("{ip}:4003").parse().context("bad ip")?;
        let payload = json!({ "msg": { "cmd": cmd, "data": data } });
        let bytes = serde_json::to_vec(&payload).context("serialize govee cmd")?;
        let _ = self.sock.send_to(&bytes, target);
        Ok(())
    }

    pub fn set_rgb(&self, ip: &str, r: u8, g: u8, b: u8) -> Result<()> {
        self.send_cmd(ip, "colorwc", json!({ "color": { "r": r, "g": g, "b": b }, "colorTemInKelvin": 0 }), )
    }

    pub fn set_brightness(&self, ip: &str, brightness_1_100: u8) -> Result<()> {
        let v = brightness_1_100.clamp(1, 100);
        self.send_cmd(ip, "brightness", json!({ "value": v }))
    }

    pub fn turn(&self, ip: &str, on: bool) -> Result<()> {
        self.send_cmd(ip, "turn", json!({ "value": if on { 1 } else { 0 } }))
    }

    pub fn get_state(&self, ip: &str) -> Result<GoveeState> {
        self.send_cmd(ip, "devStatus", json!({}))?;

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

    pub fn restore_state(&self, ip: &str, s: GoveeState) -> Result<()> {
        self.turn(ip, s.on).ok();
        self.set_brightness(ip, s.brightness).ok();
        self.set_rgb(ip, s.r, s.g, s.b).ok();
        Ok(())
    }
}
