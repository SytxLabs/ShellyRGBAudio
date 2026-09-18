use anyhow::{anyhow, Context, Result};
use reqwest::blocking::Client;
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::net::UdpSocket;
use std::time::Duration;

use super::{shape_brightness, Device, DeviceDescriptor, Frame};
use crate::color::Rgbw;

pub const DESCRIPTOR: DeviceDescriptor = DeviceDescriptor {
    type_tag: "wled",
    legacy_key: None,
    in_default_config: false,
    default_config: || serde_json::to_value(WledConfig::default()).expect("serialize wled defaults"),
    build: |entry| {
        let cfg: WledConfig = serde_json::from_value(entry.clone()).context("parse wled config")?;
        Ok(Box::new(WledDevice::new(cfg)?))
    },
};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "snake_case")]
pub enum WledProtocol {
    Warls,
    Drgb,
    Drgbw,
    #[default]
    Dnrgb,
}

impl WledProtocol {
    fn id(self) -> u8 {
        match self {
            WledProtocol::Warls => 1,
            WledProtocol::Drgb => 2,
            WledProtocol::Drgbw => 3,
            WledProtocol::Dnrgb => 4,
        }
    }
    fn bytes_per_pixel(self) -> usize {
        match self {
            WledProtocol::Warls => 4,
            WledProtocol::Drgb => 3,
            WledProtocol::Drgbw => 4,
            WledProtocol::Dnrgb => 3,
        }
    }
    fn header_len(self) -> usize { match self { WledProtocol::Dnrgb => 4, _ => 2, } }
    fn max_pixels(self) -> usize {
        match self {
            WledProtocol::Warls | WledProtocol::Drgb | WledProtocol::Drgbw => 490,
            WledProtocol::Dnrgb => 65_535,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct WledConfig {
    pub host: String,
    pub leds: Option<usize>, // How many pixels the strip has. `null` asks the controller over HTTP.
    pub segments: usize,     // How many independently colored runs the pixels are divided into. `1` makes the whole strip one color.
    pub protocol: WledProtocol,
    pub port: u16,
    pub realtime_timeout_s: u8, // WLED resumes its own effect this many seconds after the last packet. `0` would mean "never", which would strand the strip.
    pub reverse: bool,          // Flips which end of the strip is segment 0, for a strip mounted the other way round.
    pub min_brightness: u8,
    pub max_brightness: u8,
    pub brightness_gamma: f32,
    pub http_timeout_ms: u64,
    pub local_bind_addr: String,
}

impl Default for WledConfig {
    fn default() -> Self {
        Self {
            host: "192.168.1.70".to_string(),
            leds: None,
            segments: 16,
            protocol: WledProtocol::default(),
            port: 21324,
            realtime_timeout_s: 2,
            reverse: false,
            min_brightness: 1,
            max_brightness: 80,
            brightness_gamma: 0.6,
            http_timeout_ms: 3000,
            local_bind_addr: "0.0.0.0:0".to_string(),
        }
    }
}

pub struct WledDevice {
    cfg: WledConfig,
    sock: UdpSocket,
    target: String,
    http: Client,
    leds: usize,
    segments: usize,
    packet: std::sync::Mutex<Vec<u8>>, // Reused between frames so a 60 Hz strip does not allocate.
    initial: Option<WledState>,
}

#[derive(Debug, Clone, Copy)]
struct WledState {
    on: bool,
    brightness: u8,
}

impl WledDevice {
    pub fn new(cfg: WledConfig) -> Result<Self> {
        let http = Client::builder().timeout(Duration::from_millis(cfg.http_timeout_ms.max(1))).build().context("build request client")?;
        let base = normalize_base(&cfg.host);

        let leds = match cfg.leds {
            Some(n) if n > 0 => n,
            _ => fetch_led_count(&http, &base).context("could not read the LED count from the controller; set \"leds\" in the config to state it by hand")?,
        };
        let leds = leds.clamp(1, cfg.protocol.max_pixels());
        if let Some(asked) = cfg.leds && asked > cfg.protocol.max_pixels()
        {
            eprintln!("Warn: wled {}: {:?} can only address {} pixels, using that instead of {asked}.", cfg.host, cfg.protocol, cfg.protocol.max_pixels());
        }
        let segments = cfg.segments.clamp(1, leds);

        let sock = UdpSocket::bind(&cfg.local_bind_addr).with_context(|| format!("bind UDP {}", cfg.local_bind_addr))?;
        let target = format!("{}:{}", host_only(&cfg.host), cfg.port);
        sock.connect(&target).with_context(|| format!("cannot reach {target}"))?;

        let initial = fetch_state(&http, &base).ok();
        Ok(Self { cfg, sock, target, http, leds, segments, packet: std::sync::Mutex::new(Vec::new()), initial })
    }

    fn base(&self) -> String {
        normalize_base(&self.cfg.host)
    }
}

impl Device for WledDevice {
    fn name(&self) -> String { format!("wled {} ({} LEDs, {} segments)", self.cfg.host, self.leds, self.segments) }
    fn apply(&self, frame: &Frame) -> Result<()> { self.apply_segments(frame, &[Rgbw { r: frame.r, g: frame.g, b: frame.b, w: frame.w }]) }
    fn restore(&self) -> Result<()> {
        let Some(state) = self.initial else { return Ok(()) };
        let body = json!({ "on": state.on, "bri": state.brightness });
        self.http.post(format!("{}/json/state", self.base())).json(&body).send().context("restore WLED state")?.error_for_status()?;
        Ok(())
    }

    fn segment_count(&self) -> usize { self.segments }

    fn apply_segments(&self, frame: &Frame, segments: &[Rgbw]) -> Result<()> {
        if segments.is_empty() { return Ok(()); }
        let scale = shape_brightness(frame.overall, self.cfg.min_brightness, self.cfg.max_brightness, self.cfg.brightness_gamma) as f32 / 100.0;
        let mut packet = self.packet.lock().unwrap_or_else(|e| e.into_inner());
        build_packet(&mut packet, self.cfg.protocol, self.cfg.realtime_timeout_s.max(1), self.leds, segments, scale, self.cfg.reverse);
        if let Err(e) = self.sock.send(&packet) {
            eprintln!("wled {} send failed (ignored): {e}", self.target);
        }
        Ok(())
    }
}

fn build_packet(out: &mut Vec<u8>, protocol: WledProtocol, timeout_s: u8, leds: usize, segments: &[Rgbw], scale: f32, reverse: bool) {
    out.clear();
    out.reserve(protocol.header_len() + leds * protocol.bytes_per_pixel());
    out.push(protocol.id());
    out.push(timeout_s);
    if protocol == WledProtocol::Dnrgb { out.extend_from_slice(&0u16.to_be_bytes()); }

    let scale = scale.clamp(0.0, 1.0);
    for led in 0..leds {
        let idx = led * segments.len() / leds.max(1);
        let idx = if reverse { segments.len() - 1 - idx.min(segments.len() - 1) } else { idx.min(segments.len() - 1) };
        let c = segments[idx];
        let dim = |v: u8| (v as f32 * scale).round().clamp(0.0, 255.0) as u8;
        if protocol == WledProtocol::Warls { out.push(led as u8); }
        out.extend_from_slice(&[dim(c.r), dim(c.g), dim(c.b)]);
        if protocol == WledProtocol::Drgbw { out.push(dim(c.w)); }
    }
}

fn normalize_base(host: &str) -> String {
    if host.starts_with("http://") || host.starts_with("https://") {
        host.trim_end_matches('/').to_string()
    } else {
        format!("http://{}", host.trim_end_matches('/'))
    }
}

fn host_only(host: &str) -> &str {
    host.trim_start_matches("http://").trim_start_matches("https://").trim_end_matches('/').split('/').next().unwrap_or(host)
}

fn fetch_led_count(http: &Client, base: &str) -> Result<usize> {
    let v: Value = http.get(format!("{base}/json/info")).send().context("GET /json/info failed")?.error_for_status()?.json().context("parse /json/info")?;
    v.get("leds").and_then(|l| l.get("count")).and_then(|c| c.as_u64()).map(|c| c as usize).filter(|c| *c > 0).ok_or_else(|| anyhow!("/json/info carried no usable leds.count"))
}

fn fetch_state(http: &Client, base: &str) -> Result<WledState> {
    let v: Value = http.get(format!("{base}/json/state")).send().context("GET /json/state failed")?.error_for_status()?.json().context("parse /json/state")?;
    Ok(WledState {
        on: v.get("on").and_then(|x| x.as_bool()).unwrap_or(true),
        brightness: v.get("bri").and_then(|x| x.as_u64()).unwrap_or(128).min(255) as u8,
    })
}
