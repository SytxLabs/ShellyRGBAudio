use serde::{de, Deserialize, Deserializer, Serialize, Serializer};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct Rgbw { pub r: u8, pub g: u8, pub b: u8, pub w: u8 }

impl Rgbw {
    pub const fn rgb(r: u8, g: u8, b: u8) -> Self {
        Self { r, g, b, w: 0 }
    }

    pub fn parse(s: &str) -> Result<Self, String> {
        let h = s.trim().trim_start_matches('#');
        if !h.chars().all(|c| c.is_ascii_hexdigit()) {
            return Err(format!("color {s:?} is not hex"));
        }
        let nib = |i: usize| u8::from_str_radix(&h[i..i + 1], 16).map(|v| v * 17);
        let byte = |i: usize| u8::from_str_radix(&h[i..i + 2], 16);
        match h.len() {
            3 => Ok(Self { r: nib(0).unwrap(), g: nib(1).unwrap(), b: nib(2).unwrap(), w: 0 }),
            4 => Ok(Self { r: nib(0).unwrap(), g: nib(1).unwrap(), b: nib(2).unwrap(), w: nib(3).unwrap() }),
            6 => Ok(Self { r: byte(0).unwrap(), g: byte(2).unwrap(), b: byte(4).unwrap(), w: 0 }),
            8 => Ok(Self { r: byte(0).unwrap(), g: byte(2).unwrap(), b: byte(4).unwrap(), w: byte(6).unwrap() }),
            n => Err(format!("color {s:?} has {n} hex digits, expected 3, 4, 6 or 8")),
        }
    }

    pub fn to_hex(self) -> String {
        if self.w == 0 {
            format!("#{:02X}{:02X}{:02X}", self.r, self.g, self.b)
        } else {
            format!("#{:02X}{:02X}{:02X}{:02X}", self.r, self.g, self.b, self.w)
        }
    }

    fn to_f32(self) -> [f32; 4] {
        [self.r as f32, self.g as f32, self.b as f32, self.w as f32]
    }

    fn from_f32(v: [f32; 4]) -> Self {
        let c = |x: f32| x.round().clamp(0.0, 255.0) as u8;
        Self { r: c(v[0]), g: c(v[1]), b: c(v[2]), w: c(v[3]) }
    }
}

#[derive(Deserialize)]
#[serde(untagged)]
enum RgbwRepr {
    Hex(String),
    Obj {
        r: u8,
        g: u8,
        b: u8,
        #[serde(default)]
        w: u8,
    },
}

impl<'de> Deserialize<'de> for Rgbw {
    fn deserialize<D: Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
        match RgbwRepr::deserialize(d)? {
            RgbwRepr::Hex(s) => Rgbw::parse(&s).map_err(de::Error::custom),
            RgbwRepr::Obj { r, g, b, w } => Ok(Rgbw { r, g, b, w }),
        }
    }
}

impl Serialize for Rgbw {
    fn serialize<S: Serializer>(&self, s: S) -> Result<S::Ok, S::Error> {
        s.serialize_str(&self.to_hex())
    }
}

// ---------------------------------------------------------------- interpolation
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "snake_case")]
#[cfg_attr(feature = "ts", derive(ts_rs::TS), ts(export))]
pub enum Scale {
    #[default]
    Log,
    Linear,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "snake_case")]
#[cfg_attr(feature = "ts", derive(ts_rs::TS), ts(export))]
pub enum Interpolation {
    #[default]
    Srgb, // Straight lerp of the 8-bit channels. What most people expect when they place two colors next to each other.
    LinearLight, // Lerp in linear light, so the midpoint of two colors keeps their combined energy instead of getting muddy.
    Hsv, // Lerp hue along the shorter way around the color wheel. Closest to the hue ramp this program used before color stops existed.
}

const GAMMA: f32 = 2.2;

pub fn lerp_rgbw(a: Rgbw, b: Rgbw, t: f32, mode: Interpolation) -> Rgbw {
    let t = t.clamp(0.0, 1.0);
    let w = a.w as f32 + (b.w as f32 - a.w as f32) * t;
    let (r, g, bl) = match mode {
        Interpolation::Srgb => {
            let (x, y) = (a.to_f32(), b.to_f32());
            (x[0] + (y[0] - x[0]) * t, x[1] + (y[1] - x[1]) * t, x[2] + (y[2] - x[2]) * t)
        }
        Interpolation::LinearLight => {
            let lin = |c: u8| (c as f32 / 255.0).powf(GAMMA);
            let out = |v: f32| v.max(0.0).powf(1.0 / GAMMA) * 255.0;
            (out(lin(a.r) + (lin(b.r) - lin(a.r)) * t), out(lin(a.g) + (lin(b.g) - lin(a.g)) * t), out(lin(a.b) + (lin(b.b) - lin(a.b)) * t))
        }
        Interpolation::Hsv => {
            let (h1, s1, v1) = rgb_to_hsv(a);
            let (h2, s2, v2) = rgb_to_hsv(b);
            let mut dh = h2 - h1;
            if dh > 180.0 { dh -= 360.0; }
            if dh < -180.0 { dh += 360.0; }
            hsv_to_rgb((h1 + dh * t).rem_euclid(360.0), s1 + (s2 - s1) * t, v1 + (v2 - v1) * t)
        }
    };
    Rgbw::from_f32([r, g, bl, w])
}

fn rgb_to_hsv(c: Rgbw) -> (f32, f32, f32) {
    let (r, g, b) = (c.r as f32 / 255.0, c.g as f32 / 255.0, c.b as f32 / 255.0);
    let max = r.max(g).max(b);
    let min = r.min(g).min(b);
    let d = max - min;
    let h = if d <= f32::EPSILON {
        0.0
    } else if max == r {
        60.0 * (((g - b) / d) % 6.0)
    } else if max == g {
        60.0 * ((b - r) / d + 2.0)
    } else {
        60.0 * ((r - g) / d + 4.0)
    };
    (h.rem_euclid(360.0), if max <= f32::EPSILON { 0.0 } else { d / max }, max)
}

fn hsv_to_rgb(h: f32, s: f32, v: f32) -> (f32, f32, f32) {
    let (s, v) = (s.clamp(0.0, 1.0), v.clamp(0.0, 1.0));
    let c = v * s;
    let x = c * (1.0 - (((h / 60.0) % 2.0) - 1.0).abs());
    let m = v - c;
    let (r, g, b) = match (h.rem_euclid(360.0) / 60.0) as u32 {
        0 => (c, x, 0.0),
        1 => (x, c, 0.0),
        2 => (0.0, c, x),
        3 => (0.0, x, c),
        4 => (x, 0.0, c),
        _ => (c, 0.0, x),
    };
    ((r + m) * 255.0, (g + m) * 255.0, (b + m) * 255.0)
}

// ---------------------------------------------------------------- config structs

#[derive(Debug, Clone, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS), ts(export))]
pub struct ColorStop {
    pub hz: f32,
    #[cfg_attr(feature = "ts", ts(type = "string"))]
    pub color: Rgbw,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS), ts(export))]
pub struct BandConfig {
    #[serde(default = "d_band_name")]
    pub name: String,
    #[serde(default = "d_band_from")]
    pub from_hz: f32,
    #[serde(default = "d_band_to")]
    pub to_hz: f32,
    #[serde(default = "d_one")]
    pub weight: f32, // Scales how much this band pulls the mixed color and the overall level. `0` mutes the band without removing it.
    #[serde(default)]
    #[cfg_attr(feature = "ts", ts(type = "string | null"))]
    pub color: Option<Rgbw>, // Fixed color for this band. `null` means: look it up on the color stops at this band's center frequency.
}

fn d_band_name() -> String {
    "band".to_string()
}
fn d_band_from() -> f32 {
    20.0
}
fn d_band_to() -> f32 {
    20_000.0
}
fn d_one() -> f32 {
    1.0
}

impl BandConfig {
    pub fn center_hz(&self, scale: Scale) -> f32 {
        match scale {
            Scale::Log => (self.from_hz.max(1e-3) * self.to_hz.max(1e-3)).sqrt(),
            Scale::Linear => (self.from_hz + self.to_hz) * 0.5,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "snake_case")]
#[cfg_attr(feature = "ts", derive(ts_rs::TS), ts(export))]
pub enum WhiteChannel {
    #[default]
    Off, // White stays off, as it always was before it became configurable.
    MinChannel, // Pull the common part of R, G and B into the white channel. Gives a cleaner white on real RGBW strips.
    FromColor, // Keep whatever white the stops or band colors carried (`#RRGGBBWW`).
    Fixed, // Always drive `white_fixed`.
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS), ts(export))]
pub struct ColorMapConfig {
    #[serde(default = "d_stops")]
    pub stops: Vec<ColorStop>, // Color axis over frequency. A frequency between two stops gets their interpolated color.
    #[serde(default)]
    pub frequency_scale: Scale,
    #[serde(default)]
    pub interpolation: Interpolation,
    #[serde(default = "d_one")]
    pub saturation: f32,
    #[serde(default = "d_value_floor")]
    pub value_floor: f32, // Color brightness floor, applied before the per-device brightness curve.
    #[serde(default = "d_value_span")]
    pub value_span: f32,
    #[serde(default)]
    pub white_channel: WhiteChannel,
    #[serde(default)]
    pub white_fixed: u8,
    #[serde(default = "d_fallback")]
    #[cfg_attr(feature = "ts", ts(type = "string"))]
    pub fallback_color: Rgbw, // Used when nothing is playing or when no stop and no band color can answer.
}

fn d_value_floor() -> f32 {
    0.15
}
fn d_value_span() -> f32 {
    0.85
}
fn d_fallback() -> Rgbw {
    Rgbw::rgb(255, 0, 0)
}

fn d_stops() -> Vec<ColorStop> {
    vec![
        ColorStop { hz: 63.0, color: Rgbw::rgb(255, 0, 0) },
        ColorStop { hz: 632.0, color: Rgbw::rgb(0, 255, 0) },
        ColorStop { hz: 4000.0, color: Rgbw::rgb(0, 0, 255) },
    ]
}

pub fn validate_bands(bands: &[BandConfig], warnings: &mut Vec<String>) -> Vec<BandConfig> {
    let mut out: Vec<BandConfig> = Vec::with_capacity(bands.len());
    for b in bands {
        if !b.from_hz.is_finite() || !b.to_hz.is_finite() || b.from_hz < 0.0 || b.from_hz >= b.to_hz {
            warnings.push(format!("band {:?}: dropping, {} .. {} Hz is not a usable range", b.name, b.from_hz, b.to_hz));
            continue;
        }
        out.push(b.clone());
    }
    out.sort_by(|a, b| a.from_hz.partial_cmp(&b.from_hz).unwrap_or(std::cmp::Ordering::Equal));

    if out.is_empty() {
        warnings.push("no usable bands configured, falling back to the built-in bass/mid/treble split".to_string());
        return default_bands();
    }
    for w in out.windows(2) {
        if w[1].from_hz < w[0].to_hz {
            warnings.push(format!("bands {:?} and {:?} overlap, shared frequencies count toward both", w[0].name, w[1].name));
        }
    }
    out
}

pub fn default_bands() -> Vec<BandConfig> {
    vec![
        BandConfig { name: "bass".into(), from_hz: 20.0, to_hz: 200.0, weight: 1.0, color: None },
        BandConfig { name: "mid".into(), from_hz: 200.0, to_hz: 2000.0, weight: 1.0, color: None },
        BandConfig { name: "treble".into(), from_hz: 2000.0, to_hz: 8000.0, weight: 1.0, color: None },
    ]
}

impl Default for ColorMapConfig {
    fn default() -> Self {
        Self {
            stops: d_stops(),
            frequency_scale: Scale::default(),
            interpolation: Interpolation::default(),
            saturation: d_one(),
            value_floor: d_value_floor(),
            value_span: d_value_span(),
            white_channel: WhiteChannel::default(),
            white_fixed: 0,
            fallback_color: d_fallback(),
        }
    }
}

// ---------------------------------------------------------------- engine

#[derive(Debug, Clone)]
pub struct ColorEngine {
    stops: Vec<ColorStop>,
    scale: Scale,
    interp: Interpolation,
    saturation: f32,
    value_floor: f32,
    value_span: f32,
    white: WhiteChannel,
    white_fixed: u8,
    fallback: Rgbw,
    band_colors: Vec<Rgbw>, // Index aligned with the band energies coming out of the analyzer.
    band_weights: Vec<f32>,
}

impl ColorEngine {
    pub fn new(bands: &[BandConfig], map: &ColorMapConfig, warnings: &mut Vec<String>) -> Self {
        let mut stops: Vec<ColorStop> = Vec::with_capacity(map.stops.len());
        for s in &map.stops {
            if !s.hz.is_finite() || s.hz <= 0.0 {
                warnings.push(format!("color_map: dropping stop with hz={} (must be > 0)", s.hz));
                continue;
            }
            stops.push(s.clone());
        }
        stops.sort_by(|a, b| a.hz.partial_cmp(&b.hz).unwrap_or(std::cmp::Ordering::Equal));
        let before = stops.len();
        stops.dedup_by(|a, b| {
            let same = a.hz == b.hz;
            if same {
                b.color = a.color;
            }
            same
        });
        if stops.len() != before {
            warnings.push(format!("color_map: {} duplicate stop frequencies collapsed", before - stops.len()));
        }

        let mut engine = Self {
            stops,
            scale: map.frequency_scale,
            interp: map.interpolation,
            saturation: map.saturation.clamp(0.0, 1.0),
            value_floor: map.value_floor.clamp(0.0, 1.0),
            value_span: map.value_span.clamp(0.0, 1.0),
            white: map.white_channel,
            white_fixed: map.white_fixed,
            fallback: map.fallback_color,
            band_colors: Vec::new(),
            band_weights: Vec::new(),
        };

        if engine.stops.is_empty() && bands.iter().any(|b| b.color.is_none()) {
            warnings.push("color_map: no usable stops and at least one band has no color, falling back to fallback_color".to_string());
        }

        for b in bands {
            engine.band_colors.push(b.color.unwrap_or_else(|| engine.color_at(b.center_hz(engine.scale))));
            let w = if b.weight.is_finite() { b.weight.max(0.0) } else { 1.0 };
            if w != b.weight {
                warnings.push(format!("band {:?}: weight {} clamped to {w}", b.name, b.weight));
            }
            engine.band_weights.push(w);
        }
        engine
    }
    
    pub fn color_at(&self, hz: f32) -> Rgbw {
        match self.stops.len() {
            0 => self.fallback,
            1 => self.stops[0].color,
            _ => {
                let first = &self.stops[0];
                let last = self.stops.last().unwrap();
                if !hz.is_finite() {
                    return self.fallback;
                }
                if hz <= first.hz {
                    return first.color;
                }
                if hz >= last.hz {
                    return last.color;
                }
                let i = self.stops.partition_point(|s| s.hz <= hz) - 1;
                let (a, b) = (&self.stops[i], &self.stops[i + 1]);
                let t = match self.scale {
                    Scale::Log => (hz.ln() - a.hz.ln()) / (b.hz.ln() - a.hz.ln()),
                    Scale::Linear => (hz - a.hz) / (b.hz - a.hz),
                };
                lerp_rgbw(a.color, b.color, t, self.interp)
            }
        }
    }

    pub fn band_colors(&self) -> &[Rgbw] { &self.band_colors }
    pub fn band_weights(&self) -> &[f32] { &self.band_weights }

    pub fn resolve(&self, energies: &[f32], overall: f32) -> Rgbw {
        let mut acc = [0.0f32; 3];
        let mut acc_w = 0.0f32;
        let mut sum = 0.0f32;
        for (i, e) in energies.iter().enumerate() {
            let Some(&col) = self.band_colors.get(i) else { break };
            let e = if e.is_finite() { e.clamp(0.0, 1.0) * self.band_weights.get(i).copied().unwrap_or(1.0) } else { 0.0 };
            if e <= 0.0 {
                continue;
            }
            acc[0] += col.r as f32 * e;
            acc[1] += col.g as f32 * e;
            acc[2] += col.b as f32 * e;
            acc_w += col.w as f32 * e;
            sum += e;
        }

        let mut mixed = if sum <= 1e-6 {
            self.fallback
        } else {
            Rgbw::from_f32([acc[0] / sum, acc[1] / sum, acc[2] / sum, acc_w / sum])
        };

        if self.saturation > 0.0 {
            let (r, g, b) = (mixed.r as f32, mixed.g as f32, mixed.b as f32);
            let hi = r.max(g).max(b);
            let lo = r.min(g).min(b);
            let stretched = if hi > lo {
                let k = 255.0 / (hi - lo);
                [(r - lo) * k, (g - lo) * k, (b - lo) * k]
            } else {
                [self.fallback.r as f32, self.fallback.g as f32, self.fallback.b as f32]
            };
            let t = self.saturation;
            mixed = Rgbw::from_f32([r + (stretched[0] - r) * t, g + (stretched[1] - g) * t, b + (stretched[2] - b) * t, mixed.w as f32]);
        }

        let value = (self.value_floor + self.value_span * overall.clamp(0.0, 1.0)).clamp(0.0, 1.0);
        let mut out = Rgbw::from_f32([mixed.r as f32 * value, mixed.g as f32 * value, mixed.b as f32 * value, mixed.w as f32 * value]);

        out.w = match self.white {
            WhiteChannel::Off => 0,
            WhiteChannel::FromColor => out.w,
            WhiteChannel::Fixed => self.white_fixed,
            WhiteChannel::MinChannel => {
                let m = out.r.min(out.g).min(out.b);
                out.r -= m;
                out.g -= m;
                out.b -= m;
                m
            }
        };
        out
    }
}