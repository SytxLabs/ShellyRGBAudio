use anyhow::{bail, Context, Result};
use digest_auth::{AuthContext, WwwAuthenticateHeader};
use reqwest::blocking::Client;
use reqwest::header::{AUTHORIZATION, CONTENT_TYPE, WWW_AUTHENTICATE};
use serde_json::json;
use std::time::Duration;

use crate::config::{ShellyAuth, ShellyConfig, ShellyDevice};

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
        let http = Client::builder()
            .timeout(Duration::from_secs(3))
            .build()
            .context("build reqwest client")?;

        let base = normalize_base(&cfg.host);

        let api = match cfg.device {
            ShellyDevice::Rgbw2 => ApiGen::Gen1,
            ShellyDevice::PlusRgbwPm => ApiGen::Gen2,
            ShellyDevice::Auto => detect_api(&http, &base)?,
        };

        Ok(Self {
            http,
            base,
            api,
            rgbw_id: cfg.rgbw_id,
            auth: cfg.auth.clone(),
            max_brightness: cfg.max_brightness,
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


    /// Set RGBW color.
    /// - r,g,b,w are 0..255
    /// - brightness is 0..100 (mapped to Gen1 gain / Gen2 brightness)
    /// - transition_ms: Gen1 uses ms; Gen2 uses seconds (converted)
    pub fn set_rgbw(
        &self,
        r: u8,
        g: u8,
        b: u8,
        w: u8,
        brightness: u8,
        transition_ms: u32,
    ) -> Result<()> {
        match self.api {
            ApiGen::Gen1 => self.set_gen1_rgbw2(r, g, b, w, brightness, transition_ms),
            ApiGen::Gen2 => self.set_gen2_rgbw(r, g, b, w, brightness, transition_ms),
        }
    }

    // -------- Gen1: Shelly RGBW2 (color mode) --------
    fn set_gen1_rgbw2(
        &self,
        r: u8,
        g: u8,
        b: u8,
        w: u8,
        gain_0_100: u8,
        transition_ms: u32,
    ) -> Result<()> {
        // Gen1 RGBW2 Color endpoint: /color/0 with params red/green/blue/white/gain/transition/turn :contentReference[oaicite:3]{index=3}
        let url = format!(
            "{}/color/0?turn=on&red={}&green={}&blue={}&white={}&gain={}&transition={}",
            self.base, r, g, b, w, gain_0_100.clamp(1, self.max_brightness), transition_ms
        );

        let mut req = self.http.get(url);
        if let Some(a) = &self.auth {
            // Gen1 typically uses basic auth when enabled
            req = req.basic_auth(a.username.clone(), Some(a.password.clone()));
        }

        let resp = req.send().context("Gen1 /color/0 request failed")?;
        if !resp.status().is_success() {
            bail!("Gen1 Shelly error: HTTP {}", resp.status());
        }
        Ok(())
    }

    // -------- Gen2: Shelly Plus RGBW PM (RGBW.Set over JSON-RPC) --------
    fn set_gen2_rgbw(
        &self,
        r: u8,
        g: u8,
        b: u8,
        w: u8,
        brightness_0_100: u8,
        transition_ms: u32,
    ) -> Result<()> {
        // Gen2 RGBW.Set params: id, on, brightness(1..100), rgb[0..255], white(0..255), transition_duration seconds :contentReference[oaicite:4]{index=4}
        // Note: docs say at least one of on/brightness is required; we send both. :contentReference[oaicite:5]{index=5}
        let brightness = brightness_0_100.clamp(1, self.max_brightness) as u32;
        let mut transition_s = (transition_ms as f64) / 1000.0;
        if transition_s > 10.0 {
            transition_s = 9.0;
        }

        let mut params = json!({
            "id": self.rgbw_id,
            "on": true,
            "brightness": brightness,
            "rgb": [r, g, b],
            "white": w
        });

        if transition_s >= 0.5 {
            // safe range per device error: [0.5, 10800]
            params["transition_duration"] = json!(transition_s);
        }
        let _result = self.rpc_call("RGBW.Set", params)?;
        Ok(())
    }

    /// JSON-RPC call via POST /rpc :contentReference[oaicite:6]{index=6}
    fn rpc_call(&self, method: &str, params: serde_json::Value) -> Result<serde_json::Value> {
        let url = format!("{}/rpc", self.base);

        // Full JSON-RPC frame (POST /rpc with {"id":..,"method":..,"params":..}) :contentReference[oaicite:7]{index=7}
        let frame = json!({
            "id": 1,
            "method": method,
            "params": params
        });

        let body_bytes = serde_json::to_vec(&frame).context("serialize JSON-RPC frame")?;

        // 1) try without auth
        let resp = self
            .http
            .post(&url)
            .header(CONTENT_TYPE, "application/json")
            .body(body_bytes.clone())
            .send()
            .context("POST /rpc failed")?;

        // If digest auth is enabled, Shelly Gen2 uses Digest (RFC7616 style) :contentReference[oaicite:8]{index=8}
        let resp = if resp.status() == reqwest::StatusCode::UNAUTHORIZED {
            let auth = self
                .auth
                .as_ref()
                .ok_or_else(|| anyhow::anyhow!("Gen2 Shelly requires auth (401), but config.shelly.auth is null"))?;

            let www = resp
                .headers()
                .get(WWW_AUTHENTICATE)
                .ok_or_else(|| anyhow::anyhow!("401 without WWW-Authenticate header"))?
                .to_str()
                .context("WWW-Authenticate not valid utf-8")?
                .to_string();

            let mut prompt: WwwAuthenticateHeader =
                digest_auth::parse(&www).context("parse digest WWW-Authenticate")?;

            // URI must match the request path used for hashing.
            // We call POST /rpc and include the body (covers auth-int if the server asks for it).
            let ctx = AuthContext::new_post(
                auth.username.as_str(),
                auth.password.as_str(),
                "/rpc",
                Some(body_bytes.as_slice()),
            );

            let answer = prompt
                .respond(&ctx)
                .context("compute digest auth response")?
                .to_string();

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

        let v: serde_json::Value = resp.json().context("parse RPC JSON response")?;
        if let Some(err) = v.get("error") {
            bail!("Gen2 Shelly RPC returned error: {err}");
        }

        Ok(v.get("result").cloned().unwrap_or(serde_json::Value::Null))
    }

    fn get_state_gen1(&self) -> Result<ShellyRgbwState> {
        // Gen1 liefert aktuellen Status per GET /color/0 (ison/red/green/blue/white/gain) :contentReference[oaicite:2]{index=2}
        let url = format!("{}/color/0", self.base);
        let mut req = self.http.get(url);

        if let Some(a) = &self.auth {
            req = req.basic_auth(a.username.clone(), Some(a.password.clone()));
        }

        let v: serde_json::Value = req.send()?.error_for_status()?.json()?;

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
        let url = format!(
            "{}/color/0?turn={}&red={}&green={}&blue={}&white={}&gain={}&transition=0",
            self.base, turn, s.r, s.g, s.b, s.w, s.brightness
        );

        let mut req = self.http.get(url);
        if let Some(a) = &self.auth {
            req = req.basic_auth(a.username.clone(), Some(a.password.clone()));
        }

        req.send()?.error_for_status()?;
        Ok(())
    }
    fn get_state_gen2(&self) -> Result<ShellyRgbwState> {
        // RGBW.GetStatus liefert output/rgb/brightness/white :contentReference[oaicite:5]{index=5}
        let result = self.rpc_call("RGBW.GetStatus", serde_json::json!({ "id": self.rgbw_id }))?;

        let rgb = result
            .get("rgb")
            .and_then(|x| x.as_array())
            .unwrap_or(&vec![])
            .iter()
            .map(|v| v.as_u64().unwrap_or(0) as u8)
            .collect::<Vec<u8>>();

        Ok(ShellyRgbwState {
            on: result.get("output").and_then(|x| x.as_bool()).unwrap_or(false),
            r: *rgb.get(0).unwrap_or(&0),
            g: *rgb.get(1).unwrap_or(&0),
            b: *rgb.get(2).unwrap_or(&0),
            w: result.get("white").and_then(|x| x.as_u64()).unwrap_or(0).min(255) as u8,
            brightness: result
                .get("brightness")
                .and_then(|x| x.as_u64())
                .unwrap_or(0)
                .min(100) as u8,
        })
    }

    fn restore_state_gen2(&self, s: ShellyRgbwState) -> Result<()> {
        let params = serde_json::json!({
            "id": self.rgbw_id,
            "on": s.on,
            "brightness": (s.brightness.max(1).min(100)) as u32,
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

/// Detect Gen1 vs Gen2 using `/shelly` (Gen2 docs explicitly describe it as device identification incl. gen) :contentReference[oaicite:9]{index=9}
fn detect_api(http: &Client, base: &str) -> Result<ApiGen> {
    let url = format!("{}/shelly", base);
    let resp = http.get(&url).send().context("GET /shelly failed")?;
    if !resp.status().is_success() {
        // if /shelly is blocked or missing, safest fallback is Gen1 (but you can force in config)
        return Ok(ApiGen::Gen1);
    }

    let v: serde_json::Value = resp.json().context("parse /shelly json")?;
    // Gen2 usually has "gen": 2+ in /shelly response. :contentReference[oaicite:10]{index=10}
    if v.get("gen").and_then(|x| x.as_i64()).unwrap_or(1) >= 2 {
        Ok(ApiGen::Gen2)
    } else {
        Ok(ApiGen::Gen1)
    }
}
