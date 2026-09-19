use serde::{Deserialize, Serialize};

use crate::color::{BandConfig, Scale};
use crate::config::{LayoutSelector, SpatialSection};

#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
#[serde(from = "[f32; 3]", into = "[f32; 3]")]
pub struct Vec3 {
    pub x: f32,
    pub y: f32,
    pub z: f32,
}

impl Vec3 {
    pub const ZERO: Vec3 = Vec3 { x: 0.0, y: 0.0, z: 0.0 };

    pub const fn new(x: f32, y: f32, z: f32) -> Self {
        Self { x, y, z }
    }
    pub fn dot(self, o: Vec3) -> f32 {
        self.x * o.x + self.y * o.y + self.z * o.z
    }
    pub fn len(self) -> f32 {
        self.dot(self).sqrt()
    }
    pub fn normalized(self) -> Option<Vec3> {
        let l = self.len();
        if l.is_finite() && l > 1e-6 { Some(Vec3::new(self.x / l, self.y / l, self.z / l)) } else { None }
    }
    pub fn lerp(self, o: Vec3, t: f32) -> Vec3 { Vec3::new(self.x + (o.x - self.x) * t, self.y + (o.y - self.y) * t, self.z + (o.z - self.z) * t) }
    pub fn is_finite(self) -> bool { self.x.is_finite() && self.y.is_finite() && self.z.is_finite() }
    pub fn distance(self, o: Vec3) -> f32 { Vec3::new(self.x - o.x, self.y - o.y, self.z - o.z).len() }
}

impl From<[f32; 3]> for Vec3 { fn from(v: [f32; 3]) -> Self {
        Vec3::new(v[0], v[1], v[2])
    } }
impl From<Vec3> for [f32; 3] { fn from(v: Vec3) -> Self {
        [v.x, v.y, v.z]
    } }

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
#[cfg_attr(feature = "ts", derive(ts_rs::TS), ts(export))]
pub enum SpeakerRole {
    FrontLeft,
    FrontRight,
    FrontCenter,
    Lfe,
    BackLeft,
    BackRight,
    FrontLeftOfCenter,
    FrontRightOfCenter,
    BackCenter,
    SideLeft,
    SideRight,
    TopCenter,
    TopFrontLeft,
    TopFrontCenter,
    TopFrontRight,
    TopBackLeft,
    TopBackCenter,
    TopBackRight,
    Unknown,
}

#[cfg_attr(not(target_os = "windows"), allow(dead_code))]
const MASK_BITS: &[(u32, SpeakerRole)] = &[
    (0x1, SpeakerRole::FrontLeft),
    (0x2, SpeakerRole::FrontRight),
    (0x4, SpeakerRole::FrontCenter),
    (0x8, SpeakerRole::Lfe),
    (0x10, SpeakerRole::BackLeft),
    (0x20, SpeakerRole::BackRight),
    (0x40, SpeakerRole::FrontLeftOfCenter),
    (0x80, SpeakerRole::FrontRightOfCenter),
    (0x100, SpeakerRole::BackCenter),
    (0x200, SpeakerRole::SideLeft),
    (0x400, SpeakerRole::SideRight),
    (0x800, SpeakerRole::TopCenter),
    (0x1000, SpeakerRole::TopFrontLeft),
    (0x2000, SpeakerRole::TopFrontCenter),
    (0x4000, SpeakerRole::TopFrontRight),
    (0x8000, SpeakerRole::TopBackLeft),
    (0x10000, SpeakerRole::TopBackCenter),
    (0x20000, SpeakerRole::TopBackRight),
];

const DIAG: f32 = std::f32::consts::FRAC_1_SQRT_2; // 0.707, the 45-degree component used by the rear and the height speakers.

impl SpeakerRole {
    pub fn direction(self) -> Vec3 {
        match self {
            SpeakerRole::FrontLeft => Vec3::new(-0.5, 0.0, 0.866),
            SpeakerRole::FrontRight => Vec3::new(0.5, 0.0, 0.866),
            SpeakerRole::FrontCenter => Vec3::new(0.0, 0.0, 1.0),
            SpeakerRole::FrontLeftOfCenter => Vec3::new(-0.259, 0.0, 0.966),
            SpeakerRole::FrontRightOfCenter => Vec3::new(0.259, 0.0, 0.966),
            SpeakerRole::SideLeft => Vec3::new(-1.0, 0.0, 0.0),
            SpeakerRole::SideRight => Vec3::new(1.0, 0.0, 0.0),
            SpeakerRole::BackLeft => Vec3::new(-DIAG, 0.0, -DIAG),
            SpeakerRole::BackRight => Vec3::new(DIAG, 0.0, -DIAG),
            SpeakerRole::BackCenter => Vec3::new(0.0, 0.0, -1.0),
            SpeakerRole::TopCenter => Vec3::new(0.0, 1.0, 0.0),
            SpeakerRole::TopFrontLeft => Vec3::new(-0.354, DIAG, 0.612),
            SpeakerRole::TopFrontCenter => Vec3::new(0.0, DIAG, DIAG),
            SpeakerRole::TopFrontRight => Vec3::new(0.354, DIAG, 0.612),
            SpeakerRole::TopBackLeft => Vec3::new(-0.5, DIAG, -0.5),
            SpeakerRole::TopBackCenter => Vec3::new(0.0, DIAG, -DIAG),
            SpeakerRole::TopBackRight => Vec3::new(0.5, DIAG, -0.5),
            SpeakerRole::Lfe | SpeakerRole::Unknown => Vec3::ZERO, // The subwoofer is not localizable, and an unnamed channel must not be guessed at; both are spread over every device instead.
        }
    }
    pub fn is_omni(self) -> bool {
        matches!(self, SpeakerRole::Lfe | SpeakerRole::Unknown)
    }
    pub fn short_name(self) -> &'static str {
        match self {
            SpeakerRole::FrontLeft => "FL",
            SpeakerRole::FrontRight => "FR",
            SpeakerRole::FrontCenter => "FC",
            SpeakerRole::Lfe => "LFE",
            SpeakerRole::BackLeft => "BL",
            SpeakerRole::BackRight => "BR",
            SpeakerRole::FrontLeftOfCenter => "FLC",
            SpeakerRole::FrontRightOfCenter => "FRC",
            SpeakerRole::BackCenter => "BC",
            SpeakerRole::SideLeft => "SL",
            SpeakerRole::SideRight => "SR",
            SpeakerRole::TopCenter => "TC",
            SpeakerRole::TopFrontLeft => "TFL",
            SpeakerRole::TopFrontCenter => "TFC",
            SpeakerRole::TopFrontRight => "TFR",
            SpeakerRole::TopBackLeft => "TBL",
            SpeakerRole::TopBackCenter => "TBC",
            SpeakerRole::TopBackRight => "TBR",
            SpeakerRole::Unknown => "?",
        }
    }
}

#[derive(Debug, Clone)]
pub struct SpeakerLayout { pub channels: Vec<SpeakerRole>, }

#[derive(Debug, Clone, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS), ts(export))]
pub struct SpeakerPlacement {
    pub channel: usize,
    pub role: SpeakerRole,
    pub short_name: String,
    #[cfg_attr(feature = "ts", ts(type = "[number, number, number]"))]
    pub direction: Vec3,
}

impl SpeakerLayout {
    pub fn from_count(channels: usize) -> Self {
        use SpeakerRole::*;
        let roles: Vec<SpeakerRole> = match channels {
            0 => Vec::new(),
            1 => vec![Unknown], // A mono stream has no direction of its own, so it drives every light.
            2 => vec![FrontLeft, FrontRight],
            3 => vec![FrontLeft, FrontRight, Lfe],
            4 => vec![FrontLeft, FrontRight, BackLeft, BackRight],
            5 => vec![FrontLeft, FrontRight, FrontCenter, BackLeft, BackRight],
            6 => vec![FrontLeft, FrontRight, FrontCenter, Lfe, SideLeft, SideRight],
            8 => vec![FrontLeft, FrontRight, FrontCenter, Lfe, BackLeft, BackRight, SideLeft, SideRight],
            n => vec![Unknown; n],
        };
        Self { channels: roles }
    }

    #[cfg_attr(not(target_os = "windows"), allow(dead_code))]
    pub fn from_channel_mask(mask: u32, channels: usize) -> Self {
        if mask == 0 || mask.count_ones() as usize != channels {
            return Self::from_count(channels);
        }
        Self { channels: MASK_BITS.iter().filter(|(bit, _)| mask & bit != 0).map(|(_, role)| *role).collect() }
    }

    pub fn resolve(self, selector: &LayoutSelector, warnings: &mut Vec<String>) -> Self {
        let LayoutSelector::Channels(roles) = selector else { return self };
        if roles.len() != self.channels.len() {
            warnings.push(format!("spatial.layout names {} channels but the capture format has {}, using the detected layout {} instead", roles.len(), self.channels.len(), self.describe()));
            return self;
        }
        Self { channels: roles.clone() }
    }

    pub fn len(&self) -> usize {
        self.channels.len()
    }
    pub fn is_empty(&self) -> bool {
        self.channels.is_empty()
    }
    pub fn has_height(&self) -> bool { self.channels.iter().any(|r| !r.is_omni() && r.direction().y > 0.1) }

    pub fn placements(&self) -> Vec<SpeakerPlacement> {
        self.channels.iter().enumerate().filter(|(_, role)| !role.is_omni())
            .map(|(channel, role)| SpeakerPlacement { channel, role: *role, short_name: role.short_name().to_string(), direction: role.direction() }).collect()
    }

    /// `"5.1 (FL FR FC LFE SL SR)"`, for startup logging and `--list-devices`.
    pub fn describe(&self) -> String {
        let names: Vec<&str> = self.channels.iter().map(|r| r.short_name()).collect();
        let lfe = self.channels.iter().filter(|r| **r == SpeakerRole::Lfe).count();
        let label = match (self.channels.len(), lfe) {
            (0, _) => return "no channels".to_string(),
            (1, 0) => "mono".to_string(),
            (2, 0) => "stereo".to_string(),
            (n, l) => format!("{}.{l}", n - l),
        };
        format!("{label} ({})", names.join(" "))
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "snake_case")]
#[cfg_attr(feature = "ts", derive(ts_rs::TS), ts(export))]
pub enum DeviceForm {
    #[default]
    Lamp,
    Strip,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS), ts(export))]
pub struct DeviceSpatialConfig {
    pub form: DeviceForm,
    #[cfg_attr(feature = "ts", ts(type = "[number, number, number] | null"))]
    pub position: Option<Vec3>, // Lamp: the point it sits at. Strip: the center of its line. `null` leaves the device unpositioned.
    #[cfg_attr(feature = "ts", ts(type = "[number, number, number]"))]
    pub extent: Vec3, // Straight strips only: half-vector. The line runs from `position - extent` to `position + extent`.
    #[cfg_attr(feature = "ts", ts(type = "[number, number, number][] | null"))]
    pub path: Option<Vec<Vec3>>,
    pub spatiality: f32, // `0.0` follows the global mix exactly as before positions existed. `1.0` is fully directional.
    pub elevation_tilt: Option<f32>, // How strongly height maps onto frequency. `null` picks `1.0` when the layout has no height speakers, `0.0` when it has.
    pub focus: Option<f32>,          // Overrides `spatial.focus` for this one device.
    pub distance_falloff: Option<f32>, // Overrides `spatial.distance_falloff` for this one device.
}

impl Default for DeviceSpatialConfig {
    fn default() -> Self {
        Self {
            form: DeviceForm::Lamp,
            position: None,
            extent: Vec3::ZERO,
            path: None,
            spatiality: 0.0,
            elevation_tilt: None,
            focus: None,
            distance_falloff: None,
        }
    }
}

impl DeviceSpatialConfig {
    fn geometry(&self, tag: &str, warnings: &mut Vec<String>) -> Option<Vec<Vec3>> {
        if let Some(path) = &self.path {
            let usable: Vec<Vec3> = path.iter().copied().filter(|p| p.is_finite()).collect();
            if usable.len() < path.len() {
                warnings.push(format!("device {tag:?}: {} path corners are not finite numbers and were dropped", path.len() - usable.len()));
            }
            if usable.is_empty() {
                warnings.push(format!("device {tag:?}: \"path\" has no usable corners, it will follow the global mix"));
                return None;
            }
            if self.position.is_some() {
                warnings.push(format!("device {tag:?}: \"path\" is set, so \"position\" and \"extent\" are ignored"));
            }
            if self.form == DeviceForm::Lamp && usable.len() > 1 {
                warnings.push(format!("device {tag:?}: \"path\" runs through {} corners but the form is \"lamp\", using \"strip\"", usable.len()));
            }
            return Some(usable);
        }

        let position = self.position?;
        if !position.is_finite() || !self.extent.is_finite() {
            warnings.push(format!("device {tag:?}: position or extent is not a finite number, it will follow the global mix"));
            return None;
        }
        match self.form {
            DeviceForm::Lamp => Some(vec![position]),
            DeviceForm::Strip => Some(vec![
                Vec3::new(position.x - self.extent.x, position.y - self.extent.y, position.z - self.extent.z),
                Vec3::new(position.x + self.extent.x, position.y + self.extent.y, position.z + self.extent.z),
            ]),
        }
    }

    fn sample_count(&self, corners: usize, segments: usize, strip_samples: usize) -> usize {
        if self.form == DeviceForm::Lamp && corners < 2 {
            return 1;
        }
        if segments > 1 { segments } else { strip_samples }.clamp(1, 256)
    }
}

#[derive(Debug, Clone)]
pub struct DeviceSpatial {
    weights: Vec<f32>, // Per capture channel, summing to 1. The average over every sample point, which is what a single-color device shows.
    tilt: Vec<f32>,    // Per band, `1.0` when height mapping is off. Likewise, averaged.
    segment_weights: Vec<Vec<f32>>,
    segment_tilt: Vec<Vec<f32>>,
    spatiality: f32,
    pub distance_gain: f32,
    label: String,
}

impl DeviceSpatial {
    pub fn build(cfg: &DeviceSpatialConfig, layout: &SpeakerLayout, bands: &[BandConfig], section: &SpatialSection, segments: usize, tag: &str, warnings: &mut Vec<String>) -> Option<Self> {
        if !section.enabled {
            return None;
        }
        if layout.is_empty() || bands.is_empty() {
            return None;
        }
        let Some(corners) = cfg.geometry(tag, warnings) else {
            if cfg.spatiality > 0.0 && cfg.position.is_none() && cfg.path.is_none() {
                warnings.push(format!("device {tag:?}: \"spatiality\" is set but it has no \"position\" or \"path\", so it follows the global mix"));
            }
            return None;
        };
        if !cfg.spatiality.is_finite() || cfg.spatiality <= 0.0 {
            return None;
        }
        let focus = cfg.focus.unwrap_or(section.focus).clamp(0.1, 16.0);
        let tilt_amount = cfg.elevation_tilt.unwrap_or(if layout.has_height() { 0.0 } else { 1.0 }).clamp(0.0, 1.0);

        let omni_floor = section.omni_floor.clamp(0.0, 1.0);
        let points = sample_points(&corners, cfg.sample_count(corners.len(), segments, section.strip_samples));
        let (mut segment_weights, mut segment_tilt) = (Vec::new(), Vec::new());
        if segments > 1 {
            for p in &points {
                segment_weights.push(channel_weights(std::slice::from_ref(p), layout, focus, omni_floor));
                segment_tilt.push(band_tilt(std::slice::from_ref(p), bands, section, tilt_amount));
            }
        }
        Some(Self { 
            weights: channel_weights(&points, layout, focus, omni_floor),
            tilt: band_tilt(&points, bands, section, tilt_amount),
            segment_weights,
            segment_tilt,
            spatiality: cfg.spatiality.clamp(0.0, 1.0),
            distance_gain: 1.0 / (1.0 + (cfg.distance_falloff.unwrap_or(section.distance_falloff).max(0.0)) * sample_points(&corners, 1).first().copied().unwrap_or(Vec3::ZERO).len()),
            label: describe_shape(&corners)
        })
    }

    pub fn segment_count(&self) -> usize {
        self.segment_weights.len()
    }
    /// `"strip [-2.0, 0.2, 0.5] -> [-2.0, 2.2, 0.5] x0.80, FL 71% FR 29%"`, for the startup banner.
    pub fn describe(&self, layout: &SpeakerLayout) -> String {
        let mut shares: Vec<(usize, f32)> = self.weights.iter().copied().enumerate().collect();
        shares.sort_by(|a, b| b.1.total_cmp(&a.1));
        let label: Vec<_> = shares.iter().take(3).filter(|(_, w)| *w > 0.005).map(|(i, w)| format!("{} {:.0}%", layout.channels.get(*i).map_or("?", |r| r.short_name()), w * 100.0)).collect();
        let gradient = match self.segment_count() {
            0 => String::new(),
            n => format!(", {n} segment gradient"),
        };
        format!("{} x{:.2}, {}{gradient}", self.label, self.spatiality, label.join(" "))
    }
    pub fn resolve_bands(&self, global: &[f32], channels: &[Vec<f32>], out: &mut Vec<f32>) { mix_bands(&self.weights, &self.tilt, self.spatiality, global, channels, out); }
    pub fn resolve_segment_bands(&self, segment: usize, global: &[f32], channels: &[Vec<f32>], out: &mut Vec<f32>) {
        mix_bands(self.segment_weights.get(segment).unwrap_or(&self.weights), self.segment_tilt.get(segment).unwrap_or(&self.tilt), self.spatiality, global, channels, out);
    }
}

fn mix_bands(weights: &[f32], tilt: &[f32], spatiality: f32, global: &[f32], channels: &[Vec<f32>], out: &mut Vec<f32>) {
    out.clear();
    out.reserve(global.len());
    for (b, &g) in global.iter().enumerate() {
        let mut dev = 0.0f32;
        for (c, energies) in channels.iter().enumerate() {
            let Some(&e) = energies.get(b) else { continue };
            dev += weights.get(c).copied().unwrap_or(0.0) * e;
        }
        dev *= tilt.get(b).copied().unwrap_or(1.0);
        out.push((g + (dev - g) * spatiality).clamp(0.0, 1.0));
    }
}

fn sample_points(corners: &[Vec3], n: usize) -> Vec<Vec3> {
    if corners.is_empty() {
        return Vec::new();
    }
    let legs: Vec<f32> = corners.windows(2).map(|w| w[0].distance(w[1])).collect();
    let total: f32 = legs.iter().sum();
    if n <= 1 || corners.len() == 1 || !total.is_finite() || total <= 1e-6 {
        return vec![walk(corners, &legs, total * 0.5); n.max(1)];
    }
    (0..n).map(|i| walk(corners, &legs, total * i as f32 / (n - 1) as f32)).collect()
}

/// `"lamp at [2.2, 1.6, 0.0]"`, `"strip [-2.0, 0.0, 1.5] -> [-2.0, 2.4, 1.5]"`, or for a path that bends, every corner and its total length.
fn describe_shape(corners: &[Vec3]) -> String {
    let point = |p: &Vec3| format!("[{:.1}, {:.1}, {:.1}]", p.x, p.y, p.z);
    match corners {
        [] => "unplaced".to_string(),
        [p] => format!("lamp at {}", point(p)),
        [a, b] => format!("strip {} -> {}", point(a), point(b)),
        many => {
            let length: f32 = many.windows(2).map(|w| w[0].distance(w[1])).sum();
            format!("strip {} ({length:.1}m over {} legs)", many.iter().map(point).collect::<Vec<_>>().join(" -> "), many.len() - 1)
        }
    }
}

fn walk(corners: &[Vec3], legs: &[f32], distance: f32) -> Vec3 {
    let mut left = distance.max(0.0);
    for (leg, pair) in legs.iter().zip(corners.windows(2)) {
        if left <= *leg || *leg <= 1e-6 {
            let t = if *leg > 1e-6 { left / leg } else { 0.0 };
            return pair[0].lerp(pair[1], t);
        }
        left -= leg;
    }
    *corners.last().unwrap_or(&Vec3::ZERO) // Ran past the end, which rounding can do on the final sample.
}

fn channel_weights(points: &[Vec3], layout: &SpeakerLayout, focus: f32, omni_floor: f32) -> Vec<f32> {
    let n = layout.len();
    let uniform = 1.0 / n as f32;
    let directional = layout.channels.iter().filter(|r| !r.is_omni()).count();
    let budget = directional as f32 * uniform; // What the directional channels share between them.
    let mut acc = vec![0.0f32; n];

    for p in points {
        let mut raw = vec![0.0f32; n];
        let mut sum = 0.0f32;
        let dir = p.normalized();
        for (i, role) in layout.channels.iter().enumerate() {
            if role.is_omni() || dir.is_none() {
                continue;
            }
            raw[i] = dir.unwrap().dot(role.direction()).max(0.0).powf(focus);
            sum += raw[i];
        }

        for (i, role) in layout.channels.iter().enumerate() {
            raw[i] = match (role.is_omni(), sum > 1e-6) {
                (true, _) => uniform,
                (false, false) => budget / directional.max(1) as f32,
                (false, true) => raw[i] / sum * budget,
            };
        }
        for (a, w) in acc.iter_mut().zip(raw) {
            *a += w;
        }
    }

    let count = points.len().max(1) as f32;
    for a in acc.iter_mut() { *a = (*a / count) * (1.0 - omni_floor) + uniform * omni_floor; }
    acc
}

fn band_tilt(points: &[Vec3], bands: &[BandConfig], section: &SpatialSection, amount: f32) -> Vec<f32> {
    if amount <= 0.0 {
        return vec![1.0; bands.len()];
    }
    let centers: Vec<f32> = bands.iter().map(|b| b.center_hz(Scale::Log).max(1e-3).ln()).collect();
    let lo = centers.iter().copied().fold(f32::INFINITY, f32::min);
    let hi = centers.iter().copied().fold(f32::NEG_INFINITY, f32::max);
    if !(hi - lo).is_finite() || hi - lo <= 1e-6 {
        return vec![1.0; bands.len()];
    }

    let (y_lo, y_hi) = (section.room.min.y, section.room.max.y);
    let span = y_hi - y_lo;
    if !span.is_finite() || span.abs() <= 1e-6 {
        return vec![1.0; bands.len()];
    }
    let sigma = section.height_sharpness.clamp(0.05, 4.0);

    let mut acc = vec![0.0f32; bands.len()];
    for p in points {
        let height = ((p.y - y_lo) / span).clamp(0.0, 1.0);
        for (i, center) in centers.iter().enumerate() {
            let d = (height - ((center - lo) / (hi - lo))) / sigma;
            acc[i] += 1.0 + (( -(d * d)).exp() - 1.0) * amount;
        }
    }

    let count = points.len().max(1) as f32;
    for a in acc.iter_mut() {
        *a /= count;
    }
    acc
}
