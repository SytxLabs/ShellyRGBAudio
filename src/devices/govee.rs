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

/// All Govee LAN lights share one UDP socket, because the protocol requires the local port 4002, and it can only be bound once per process.
static CLIENT: OnceLock<std::result::Result<SharedSocket, String>> = OnceLock::new();

fn client(bind: &str, port: u16, read_timeout_ms: u64) -> Result<Arc<GoveeLan>> {
    let shared = CLIENT
        .get_or_init(|| {
            GoveeLan::new(bind, port, read_timeout_ms).map(|lan| SharedSocket { lan: Arc::new(lan), bind: bind.to_string(), port, read_timeout_ms }).map_err(|e| format!("{e:#}"))
        })
        .clone()
        .map_err(|e| anyhow!(e))?;

    if shared.bind != bind || shared.port != port || shared.read_timeout_ms != read_timeout_ms {
        eprintln!("Warn: govee_lan socket is already bound to {}:{} (read timeout {} ms); ignoring the differing local settings of this entry.", shared.bind, shared.port, shared.read_timeout_ms);
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
        let lan = client(&cfg.local_bind_addr, cfg.local_port, cfg.read_timeout_ms)?;
        let initial = lan.get_state(&cfg.ip, cfg.remote_port).ok();
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
        let port = self.cfg.remote_port;
        self.lan.turn(&self.cfg.ip, port, true).ok();
        self.lan.set_brightness(&self.cfg.ip, port, bri).ok();
        self.lan.set_rgb(&self.cfg.ip, port, frame.r, frame.g, frame.b, self.cfg.color_temp_kelvin).ok();
        Ok(())
    }

    fn restore(&self) -> Result<()> {
        if let Some(st) = self.initial {
            self.lan.restore_state(&self.cfg.ip, self.cfg.remote_port, st, self.cfg.color_temp_kelvin)?;
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
