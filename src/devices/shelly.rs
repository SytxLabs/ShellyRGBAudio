use anyhow::{bail, Context, Result};
use digest_auth::{AuthContext, WwwAuthenticateHeader};
use reqwest::blocking::Client;
use reqwest::header::{AUTHORIZATION, CONTENT_TYPE, WWW_AUTHENTICATE};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::time::Duration;

use super::{shape_brightness, Device, DeviceDescriptor, Frame};

pub const DESCRIPTOR: DeviceDescriptor = DeviceDescriptor {
    type_tag: "shelly",
    legacy_key: Some("shellys"),
    in_default_config: true,
    default_config: || serde_json::to_value(ShellyConfig::default()).expect("serialize shelly defaults"),
    build: |entry| {
        let cfg: ShellyConfig = serde_json::from_value(entry.clone()).context("parse shelly config")?;
        Ok(Box::new(ShellyLight::new(cfg)?))
    },
};

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct ShellyConfig {
    pub host: String,
    pub device: ShellyModel,
    pub min_brightness: u8,
    pub max_brightness: u8,
    pub brightness_gamma: f32,
    pub rgbw_id: u8,
    pub auth: Option<ShellyAuth>,
    pub http_timeout_ms: u64,
    pub gen2_min_transition_ms: u64,
    pub gen2_max_transition_s: f64,
}

impl Default for ShellyConfig {
    fn default() -> Self {
        Self {
            host: "192.168.178.50".to_string(),
            device: ShellyModel::Auto,
            min_brightness: 1,
            max_brightness: 80,
            brightness_gamma: 0.6,
            rgbw_id: 0,
            auth: None,
            http_timeout_ms: 3000,
            gen2_min_transition_ms: 500,
            gen2_max_transition_s: 10600.0,
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
pub enum ShellyModel {
    Auto,
    Rgbw2,        // Gen1
    PlusRgbwPm,   // Gen2
}

/// A configured Shelly light: the HTTP controller plus the state it had when the program started.
pub struct ShellyLight {
    cfg: ShellyConfig,
    ctrl: ShellyController,
    initial: Option<ShellyRgbwState>,
}

impl ShellyLight {
    pub fn new(cfg: ShellyConfig) -> Result<Self> {
        let ctrl = ShellyController::new(&cfg)?;
        let initial = ctrl.get_state().ok();
        Ok(Self { cfg, ctrl, initial })
    }
}

impl Device for ShellyLight {
    fn name(&self) -> String {
        format!("shelly {}", self.cfg.host)
    }

    fn apply(&self, frame: &Frame) -> Result<()> {
        self.ctrl.set_rgbw(frame.r, frame.g, frame.b, frame.w, shape_brightness(frame.overall, self.cfg.min_brightness, self.cfg.max_brightness, self.cfg.brightness_gamma), frame.transition_ms)
    }

    fn restore(&self) -> Result<()> {
        if let Some(st) = self.initial {
            self.ctrl.restore_state(st)?;
        }
        Ok(())
    }
}

#[derive(Debug, Clone, Copy)]
enum ApiGen {
    Gen1,
    Gen2,
}

pub struct ShellyController {
    http: Client,
    base: String,
    api: ApiGen,
    rgbw_id: u8,
    auth: Option<ShellyAuth>, // OWNED strings => no lifetime issues
    max_brightness: u8,
    gen2_min_transition_s: f64,
    gen2_max_transition_s: f64,
}

#[derive(Debug, Clone, Copy)]
pub struct ShellyRgbwState {
    pub on: bool,
    pub r: u8,
    pub g: u8,
    pub b: u8,
    pub w: u8,
    pub brightness: u8, // 0..100 (Gen1 gain / Gen2 brightness)
}

impl ShellyController {
    pub fn new(cfg: &ShellyConfig) -> Result<Self> {
        let http = Client::builder().timeout(Duration::from_millis(cfg.http_timeout_ms.max(1))).build().context("build request client")?;
        let base = normalize_base(&cfg.host);
        let api = match cfg.device {
            ShellyModel::Rgbw2 => ApiGen::Gen1,
            ShellyModel::PlusRgbwPm => ApiGen::Gen2,
            ShellyModel::Auto => detect_api(&http, &base)?,
        };
        Ok(Self {
            http,
            base,
            api,
            rgbw_id: cfg.rgbw_id,
            auth: cfg.auth.clone(),
            max_brightness: cfg.max_brightness,
            gen2_min_transition_s: cfg.gen2_min_transition_ms as f64 / 1000.0,
            gen2_max_transition_s: cfg.gen2_max_transition_s,
        })
    }

    pub fn get_state(&self) -> Result<ShellyRgbwState> {
        match self.api {
            ApiGen::Gen1 => self.get_state_gen1(),
            ApiGen::Gen2 => self.get_state_gen2(),
        }
    }

    pub fn restore_state(&self, s: ShellyRgbwState) -> Result<()> {
        match self.api {
            ApiGen::Gen1 => self.restore_state_gen1(s),
            ApiGen::Gen2 => self.restore_state_gen2(s),
        }
    }


    pub fn set_rgbw(&self, r: u8, g: u8, b: u8, w: u8, brightness: u8, transition_ms: u32, ) -> Result<()> {
        match self.api {
            ApiGen::Gen1 => self.set_gen1_rgbw2(r, g, b, w, brightness, transition_ms),
            ApiGen::Gen2 => self.set_gen2_rgbw(r, g, b, w, brightness, transition_ms),
        }
    }

    // -------- Gen1: Shelly RGBW2 (color mode) --------
    fn set_gen1_rgbw2(&self, r: u8, g: u8, b: u8, w: u8, gain_0_100: u8, transition_ms: u32, ) -> Result<()> {
        let url = format!("{}/color/0?turn=on&red={}&green={}&blue={}&white={}&gain={}&transition={}", self.base, r, g, b, w, gain_0_100.clamp(1, self.max_brightness), transition_ms);

        let mut req = self.http.get(url);
        if let Some(a) = &self.auth {
            req = req.basic_auth(a.username.clone(), Some(a.password.clone()));
        }
        // If timeout log and not Crash
        let resp = match req.send() {
            Ok(r) => r,
            Err(e) => {
                if e.is_timeout() {
                    eprintln!("Gen1 Shelly timeout (ignored): {e}");
                    return Ok(());
                }
                return Err(e).context("Gen1 /color/0 request failed");
            }
        };
        if !resp.status().is_success() {
            eprintln!("Gen1 Shelly HTTP error (ignored): {}", resp.status());
            return Ok(());
        }
        Ok(())
    }

    // -------- Gen2: Shelly Plus RGBW PM (RGBW.Set over JSON-RPC) --------
    fn set_gen2_rgbw(&self, r: u8, g: u8, b: u8, w: u8, brightness_0_100: u8, transition_ms: u32, ) -> Result<()> {
        let brightness = brightness_0_100.clamp(1, self.max_brightness) as u32;
        let transition_s = ((transition_ms as f64) / 1000.0).min(self.gen2_max_transition_s);

        let mut params = json!({
            "id": self.rgbw_id,
            "on": true,
            "brightness": brightness,
            "rgb": [r, g, b],
            "white": w
        });

        if transition_s >= self.gen2_min_transition_s {
            params["transition_duration"] = json!(transition_s);
        }
        let _result = self.rpc_call("RGBW.Set", params)?;
        Ok(())
    }

    fn rpc_call(&self, method: &str, params: Value) -> Result<Value> {
        let url = format!("{}/rpc", self.base);
        let frame = json!({
            "id": 1,
            "method": method,
            "params": params
        });
        let body_bytes = serde_json::to_vec(&frame).context("serialize JSON-RPC frame")?;

        // 1) try without auth
        let resp = match self.http.post(&url).header(CONTENT_TYPE, "application/json").body(body_bytes.clone()).send()
        {
            Ok(r) => r,
            Err(e) => {
                if e.is_timeout() {
                    eprintln!("Gen2 Shelly timeout (ignored) POST /rpc: {e}");
                    return Ok(Value::Null);
                }
                if e.is_connect() {
                    eprintln!("Gen2 Shelly connect error (ignored) POST /rpc: {e}");
                    return Ok(Value::Null);
                }
                if e.status() == Some(reqwest::StatusCode::UNAUTHORIZED) {
                    return Ok(Value::Null);
                }
                return Err(e).context("POST /rpc failed");
            }
        };


        let resp = if resp.status() == reqwest::StatusCode::UNAUTHORIZED {
            let auth = self.auth.as_ref().ok_or_else(|| anyhow::anyhow!("Gen2 Shelly requires auth (401), but config.shelly.auth is null"))?;
            let www = resp
                .headers()
                .get(WWW_AUTHENTICATE)
                .ok_or_else(|| anyhow::anyhow!("401 without WWW-Authenticate header"))?
                .to_str()
                .context("WWW-Authenticate not valid utf-8")?
                .to_string();

            let mut prompt: WwwAuthenticateHeader = digest_auth::parse(&www).context("parse digest WWW-Authenticate")?;

            let ctx = AuthContext::new_post(auth.username.as_str(), auth.password.as_str(), "/rpc", Some(body_bytes.as_slice()), );
            let answer = prompt.respond(&ctx).context("compute digest auth response")?.to_string();
            self.http
                .post(&url)
                .header(CONTENT_TYPE, "application/json")
                .header(AUTHORIZATION, answer)
                .body(body_bytes)
                .send()
                .context("POST /rpc with digest auth failed")?
        } else {
            resp
        };

        if !resp.status().is_success() {
            bail!("Gen2 Shelly RPC error: HTTP {}", resp.status());
        }

        let v: Value = resp.json().context("parse RPC JSON response")?;
        if let Some(err) = v.get("error") {
            bail!("Gen2 Shelly RPC returned error: {err}");
        }

        Ok(v.get("result").cloned().unwrap_or(Value::Null))
    }

    fn get_state_gen1(&self) -> Result<ShellyRgbwState> {
        let url = format!("{}/color/0", self.base);
        let mut req = self.http.get(url);
        if let Some(a) = &self.auth {
            req = req.basic_auth(a.username.clone(), Some(a.password.clone()));
        }

        let v: Value = req.send()?.error_for_status()?.json()?;

        Ok(ShellyRgbwState {
            on: v.get("ison").and_then(|x| x.as_bool()).unwrap_or(false),
            r: v.get("red").and_then(|x| x.as_u64()).unwrap_or(0) as u8,
            g: v.get("green").and_then(|x| x.as_u64()).unwrap_or(0) as u8,
            b: v.get("blue").and_then(|x| x.as_u64()).unwrap_or(0) as u8,
            w: v.get("white").and_then(|x| x.as_u64()).unwrap_or(0) as u8,
            brightness: v.get("gain").and_then(|x| x.as_u64()).unwrap_or(0).min(100) as u8,
        })
    }

    fn restore_state_gen1(&self, s: ShellyRgbwState) -> Result<()> {
        let turn = if s.on { "on" } else { "off" };
        let url = format!("{}/color/0?turn={}&red={}&green={}&blue={}&white={}&gain={}&transition=0", self.base, turn, s.r, s.g, s.b, s.w, s.brightness);

        let mut req = self.http.get(url);
        if let Some(a) = &self.auth {
            req = req.basic_auth(a.username.clone(), Some(a.password.clone()));
        }
        req.send()?.error_for_status()?;
        Ok(())
    }
    fn get_state_gen2(&self) -> Result<ShellyRgbwState> {
        let result = self.rpc_call("RGBW.GetStatus", json!({ "id": self.rgbw_id }))?;

        let rgb = result
            .get("rgb")
            .and_then(|x| x.as_array())
            .unwrap_or(&vec![])
            .iter()
            .map(|v| v.as_u64().unwrap_or(0) as u8)
            .collect::<Vec<u8>>();

        Ok(ShellyRgbwState {
            on: result.get("output").and_then(|x| x.as_bool()).unwrap_or(false),
            r: *rgb.first().unwrap_or(&0),
            g: *rgb.get(1).unwrap_or(&0),
            b: *rgb.get(2).unwrap_or(&0),
            w: result.get("white").and_then(|x| x.as_u64()).unwrap_or(0).min(255) as u8,
            brightness: result.get("brightness").and_then(|x| x.as_u64()).unwrap_or(0).min(100) as u8,
        })
    }

    fn restore_state_gen2(&self, s: ShellyRgbwState) -> Result<()> {
        let params = json!({
            "id": self.rgbw_id,
            "on": s.on,
            "brightness": s.brightness.clamp(1, 100) as u32,
            "rgb": [s.r, s.g, s.b],
            "white": s.w
        });
        let _ = self.rpc_call("RGBW.Set", params)?;
        Ok(())
    }
}

fn normalize_base(host: &str) -> String {
    if host.starts_with("http://") || host.starts_with("https://") {
        host.trim_end_matches('/').to_string()
    } else {
        format!("http://{}", host.trim_end_matches('/'))
    }
}

fn detect_api(http: &Client, base: &str) -> Result<ApiGen> {
    let url = format!("{}/shelly", base);
    let resp = http.get(&url).send().context("GET /shelly failed")?;
    if !resp.status().is_success() {
        return Ok(ApiGen::Gen1);
    }
    let v: Value = resp.json().context("parse /shelly json")?;
    if v.get("gen").and_then(|x| x.as_i64()).unwrap_or(1) >= 2 {
        Ok(ApiGen::Gen2)
    } else {
        Ok(ApiGen::Gen1)
    }
}
