use anyhow::{bail, Context, Result};
use reqwest::blocking::Client;
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::{
    sync::{mpsc, Mutex},
    thread,
    time::{Duration, Instant},
};

use super::{shape_brightness, Device, DeviceDescriptor, Frame};
use crate::color::Rgbw;
use crate::{log_info, log_warn};

pub const DESCRIPTOR: DeviceDescriptor = DeviceDescriptor {
    type_tag: "hue",
    legacy_key: None,
    in_default_config: false,
    default_config: || serde_json::to_value(HueConfig::default()).expect("serialize hue defaults"),
    build: |entry| {
        let cfg: HueConfig = serde_json::from_value(entry.clone()).context("parse hue config")?;
        Ok(Box::new(HueLights::new(cfg)?))
    },
};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct HueLightInfo { pub id: String, pub name: String, }

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct HueConfig {
    pub bridge: String,          // IP or hostname of the Hue bridge.
    pub application_key: String, // What the bridge hands out once the link button has been pressed. Without it every request is rejected.
    pub lights: Vec<String>,     // Resource ids of the `light` resources to drive. Several lights act as the segments of one strip.
    pub min_brightness: u8,
    pub max_brightness: u8,
    pub brightness_gamma: f32,
    pub http_timeout_ms: u64,
    pub max_updates_per_second: f32, // The bridge drops what comes in faster than it can forward over Zigbee, so frames are thinned out to this.
    pub transitions: bool,           // Whether the fade the engine asks for is passed on. Off makes a light jump, which is snappier but harsher.
    pub verify_tls: bool,            // The bridge answers HTTPS with a certificate signed for its own id, which no store knows. Off skips that check.
}

impl Default for HueConfig {
    fn default() -> Self {
        Self {
            bridge: "192.168.1.80".to_string(),
            application_key: String::new(),
            lights: Vec::new(),
            min_brightness: 1,
            max_brightness: 80,
            brightness_gamma: 0.6,
            http_timeout_ms: 2000,
            max_updates_per_second: 10.0,
            transitions: true,
            verify_tls: false,
        }
    }
}

pub struct HueLights {
    cfg: HueConfig,
    http: Client,
    base: String,
    labels: Vec<String>,
    initial: Vec<(String, HueState)>,
    tx: Option<mpsc::Sender<Vec<(String, Value)>>>,
    worker: Option<thread::JoinHandle<()>>,
    last_on: Mutex<Option<bool>>,
}

#[derive(Debug, Clone, Copy)]
struct HueState {
    on: bool,
    brightness: f32,
    xy: Option<(f32, f32)>,
    mirek: Option<u16>,
}

impl HueLights {
    pub fn new(cfg: HueConfig) -> Result<Self> {
        if cfg.application_key.trim().is_empty() {
            bail!("hue {}: \"application_key\" is empty. Press the link button on the bridge and pair, that is what hands the key out.", cfg.bridge);
        }
        let http = client(&cfg)?;
        let base = base_url(&cfg.bridge);

        let found = fetch_lights(&http, &base, &cfg.application_key).context("could not list the lights of the bridge")?;
        let wanted: Vec<String> = cfg.lights.iter().map(|id| id.trim().to_string()).filter(|id| !id.is_empty()).collect();
        if wanted.is_empty() {
            bail!("hue {}: no light selected in \"lights\". The bridge offers: {}", cfg.bridge, describe(&found));
        }

        let mut labels = Vec::with_capacity(wanted.len());
        let mut initial = Vec::with_capacity(wanted.len());
        for id in &wanted {
            let Some(light) = found.iter().find(|l| &l.id == id) else {
                bail!("hue {}: no light with the id {id:?}. The bridge offers: {}", cfg.bridge, describe(&found));
            };
            labels.push(light.name.clone());
            initial.push((id.clone(), light.state));
        }

        let (tx, rx) = mpsc::channel::<Vec<(String, Value)>>();
        let worker = spawn_worker(rx, client(&cfg)?, base.clone(), cfg.application_key.clone(), interval(cfg.max_updates_per_second));

        Ok(Self { cfg: HueConfig { lights: wanted, ..cfg }, http, base, labels, initial, tx: Some(tx), worker: Some(worker), last_on: Mutex::new(None) })
    }

    fn put(&self, id: &str, body: &Value) -> Result<()> {
        let response = self
            .http
            .put(format!("{}/resource/light/{id}", self.base))
            .header("hue-application-key", &self.cfg.application_key)
            .json(body)
            .send()
            .with_context(|| format!("PUT light {id}"))?;
        if !response.status().is_success() {
            bail!("bridge answered HTTP {} for light {id}", response.status());
        }
        Ok(())
    }
}

impl Device for HueLights {
    fn name(&self) -> String {
        format!("hue {} ({})", self.cfg.bridge, self.labels.join(", "))
    }
    fn apply(&self, frame: &Frame) -> Result<()> { self.apply_segments(frame, &[Rgbw { r: frame.r, g: frame.g, b: frame.b, w: frame.w }]) }
    fn restore(&self) -> Result<()> {
        for (id, state) in &self.initial {
            let mut body = json!({ "on": { "on": state.on }, "dimming": { "brightness": state.brightness.clamp(0.0, 100.0) } });
            if let Some((x, y)) = state.xy { body["color"] = json!({ "xy": { "x": x, "y": y } }); }
            else if let Some(mirek) = state.mirek { body["color_temperature"] = json!({ "mirek": mirek }); }

            if let Err(e) = self.put(id, &body) { log_warn!("hue {}: could not restore light {id} (ignored): {e:#}", self.cfg.bridge); }
        }
        Ok(())
    }
    fn segment_count(&self) -> usize { self.cfg.lights.len().max(1) }
    fn apply_segments(&self, frame: &Frame, segments: &[Rgbw]) -> Result<()> {
        let Some(tx) = &self.tx else { return Ok(()) };
        if segments.is_empty() { return Ok(()); }
        let shaped = shape_brightness(frame.overall, self.cfg.min_brightness, self.cfg.max_brightness, self.cfg.brightness_gamma) as f32;
        let on = shaped > 0.0;
        let send_on = {
            let mut last = self.last_on.lock().unwrap_or_else(|e| e.into_inner());
            let changed = *last != Some(on);
            *last = Some(on);
            changed
        };

        let duration = if self.cfg.transitions { Some(frame.transition_ms) } else { None };
        let _ = tx.send(self.cfg.lights.iter().enumerate().map(|(i, id)| {
            let at = (i * segments.len() / self.cfg.lights.len().max(1)).min(segments.len() - 1);
            (id.clone(), body(segments[at], shaped, duration, send_on.then_some(on)))
        }).collect());
        Ok(())
    }
}

impl Drop for HueLights {
    fn drop(&mut self) {
        drop(self.tx.take());
        if let Some(worker) = self.worker.take() { let _ = worker.join(); }
    }
}

fn spawn_worker(rx: mpsc::Receiver<Vec<(String, Value)>>, http: Client, base: String, key: String, interval: Duration) -> thread::JoinHandle<()> {
    thread::Builder::new().name("hue-sender".to_string()).spawn(move || {
        let mut announced_failure = false;
        let mut next_at = Instant::now();

        while let Ok(mut update) = rx.recv() {
            let now = Instant::now();
            if now < next_at {
                thread::sleep(next_at - now);
            }
            while let Ok(newer) = rx.try_recv() {
                update = newer;
            }
            next_at = Instant::now() + interval;

            for (id, body) in &update {
                let sent = http.put(format!("{base}/resource/light/{id}")).header("hue-application-key", &key).json(body)
                    .send().map_err(|e| e.to_string()).and_then(|r| if r.status().is_success() { Ok(()) } else { Err(format!("HTTP {}", r.status())) });
                match sent {
                    Ok(()) => {
                        if announced_failure {
                            log_info!("hue {base}: light {id} answers again");
                            announced_failure = false;
                        }
                    }
                    Err(e) if !announced_failure => {
                        log_warn!("hue {base}: light {id} did not take the update (ignored): {e}");
                        announced_failure = true;
                    }
                    Err(_) => {}
                }
            }
        }
    }).expect("spawn the hue sender thread")
}

fn interval(per_second: f32) -> Duration { Duration::from_secs_f32(1.0 / per_second.clamp(0.5, 20.0)) }

fn body(color: Rgbw, brightness: f32, duration_ms: Option<u32>, on: Option<bool>) -> Value {
    let (x, y) = rgb_to_xy(color);
    let value = [color.r, color.g, color.b, color.w].into_iter().max().unwrap_or(0) as f32 / 255.0;
    let mut body = json!({"dimming": { "brightness": (brightness * value).clamp(1.0, 100.0) },"color": { "xy": { "x": x, "y": y } },});
    if let Some(on) = on {
        body["on"] = json!({ "on": on });
    }
    if let Some(ms) = duration_ms {
        body["dynamics"] = json!({ "duration": (ms / 100) * 100 });
    }
    body
}

fn rgb_to_xy(color: Rgbw) -> (f32, f32) {
    let linear = |c: u8| {
        let c = c as f32 / 255.0;
        if c > 0.04045 { ((c + 0.055) / 1.055).powf(2.4) } else { c / 12.92 }
    };
    let white = linear(color.w);
    let (r, g, b) = (linear(color.r) + white, linear(color.g) + white, linear(color.b) + white);

    let x = r * 0.649_926 + g * 0.103_455 + b * 0.197_109;
    let y = r * 0.234_327 + g * 0.743_075 + b * 0.022_538;
    let z = g * 0.053_077 + b * 1.035_763;

    let sum = x + y + z;
    if sum <= f32::EPSILON { return (0.3127, 0.3290); }
    (x / sum, y / sum)
}

fn client(cfg: &HueConfig) -> Result<Client> {
    Client::builder().timeout(Duration::from_millis(cfg.http_timeout_ms.max(1))).tls_danger_accept_invalid_certs(!cfg.verify_tls).build().context("build request client")
}

fn base_url(bridge: &str) -> String {
    let host = bridge.trim().trim_start_matches("https://").trim_start_matches("http://").trim_end_matches('/');
    format!("https://{host}/clip/v2")
}

struct FoundLight { id: String, name: String, state: HueState, }

fn describe(lights: &[FoundLight]) -> String {
    if lights.is_empty() { return "no lights at all".to_string(); }
    lights.iter().map(|l| format!("{:?} ({})", l.name, l.id)).collect::<Vec<_>>().join(", ")
}

fn fetch_lights(http: &Client, base: &str, key: &str) -> Result<Vec<FoundLight>> {
    let response = http.get(format!("{base}/resource/light")).header("hue-application-key", key).send().context("GET /resource/light failed")?;
    if response.status() == reqwest::StatusCode::UNAUTHORIZED || response.status() == reqwest::StatusCode::FORBIDDEN {
        bail!("the bridge rejected the application key. Pair again to get a new one.");
    }
    let body: Value = response.error_for_status()?.json().context("parse /resource/light")?;

    let mut out = Vec::new();
    for light in body.get("data").and_then(|d| d.as_array()).map(|a| a.as_slice()).unwrap_or_default() {
        let Some(id) = light.get("id").and_then(|v| v.as_str()) else { continue };
        let name = light.get("metadata").and_then(|m| m.get("name")).and_then(|v| v.as_str()).unwrap_or(id).to_string();
        out.push(FoundLight { id: id.to_string(), name, state: read_state(light) });
    }
    Ok(out)
}

fn read_state(light: &Value) -> HueState {
    let xy = light.get("color").and_then(|c| c.get("xy")).and_then(|xy| {
        Some((xy.get("x").and_then(|v| v.as_f64())? as f32, xy.get("y").and_then(|v| v.as_f64())? as f32))
    });
    HueState {
        on: light.get("on").and_then(|o| o.get("on")).and_then(|v| v.as_bool()).unwrap_or(true),
        brightness: light.get("dimming").and_then(|d| d.get("brightness")).and_then(|v| v.as_f64()).unwrap_or(100.0) as f32,
        xy,
        mirek: light.get("color_temperature").and_then(|c| c.get("mirek")).and_then(|v| v.as_u64()).map(|v| v as u16),
    }
}

pub fn list_lights(bridge: &str, application_key: &str) -> Result<Vec<HueLightInfo>> {
    let cfg = HueConfig { bridge: bridge.to_string(), application_key: application_key.to_string(), ..HueConfig::default() };
    Ok(fetch_lights(&client(&cfg)?, &base_url(bridge), application_key)?.into_iter().map(|l| HueLightInfo { id: l.id, name: l.name }).collect())
}

pub fn pair(bridge: &str) -> Result<String> {
    let cfg = HueConfig { bridge: bridge.to_string(), http_timeout_ms: 5000, ..HueConfig::default() };
    let http = client(&cfg)?;
    let host = base_url(bridge).trim_end_matches("/clip/v2").to_string();

    let body: Value = http.post(format!("{host}/api")).json(&json!({ "devicetype": "shellyrgbaudio#desktop", "generateclientkey": true })).send()
        .context("POST /api failed; is the bridge address right?")?.error_for_status()?.json().context("parse the pairing answer")?;
    let first = body.get(0).cloned().unwrap_or(Value::Null);
    if let Some(key) = first.get("success").and_then(|s| s.get("username")).and_then(|v| v.as_str()) {
        return Ok(key.to_string());
    }
    let reason = first.get("error").and_then(|e| e.get("description")).and_then(|v| v.as_str()).unwrap_or("the bridge gave no reason").to_string();
    bail!("pairing was refused: {reason}");
}
