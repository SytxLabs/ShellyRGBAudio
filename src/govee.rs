use anyhow::{Context, Result};
use serde_json::json;
use std::net::{SocketAddr, UdpSocket};
use std::time::Duration;

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

    fn send_cmd(&self, ip: &str, cmd: &str, data: serde_json::Value) -> Result<()> {
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
        let v: serde_json::Value = serde_json::from_slice(&buf[..n]).context("parse devStatus")?;
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
