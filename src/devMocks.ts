import type { AppConfig, AppInfo, AppPrefs, AudioDeviceInfo, ConfigPayload, DeviceTypeInfo, EngineStatus, LogLine, SpeakerPlacement } from "./bindings";

export const usingMocks = import.meta.env.DEV && typeof window !== "undefined" && !("__TAURI_INTERNALS__" in window);

const config: AppConfig = {
  audio: {
    device: { type: "default" },
    apps: { enabled: false, mode: "include", include_process_tree: true, rescan_ms: 3000, sample_rate: 48000, channels: 2, groups: [] },
    fft_size: 1024,
    hop_size: 1024,
    window: "hann",
    downmix: "average",
    buffer_duration_hns: 200000,
    silence_timeout_ms: 2000,
    silence_fade_ms: 800,
    silence_brightness: 0,
  },
  bands: [
    { name: "bass", from_hz: 20, to_hz: 200, weight: 1, color: null },
    { name: "mid", from_hz: 200, to_hz: 2000, weight: 1, color: null },
    { name: "treble", from_hz: 2000, to_hz: 8000, weight: 1, color: null },
  ],
  color_map: {
    stops: [
      { hz: 63, color: "#FF0000" },
      { hz: 632, color: "#00FF00" },
      { hz: 4000, color: "#0000FF" },
    ],
    frequency_scale: "log",
    interpolation: "srgb",
    saturation: 1,
    value_floor: 0.15,
    value_span: 0.85,
    white_channel: "off",
    white_fixed: 0,
    fallback_color: "#FF0000",
  },
  dynamics: {
    normalize: "shared",
    level_source: "peak",
    log_offset: 1,
    peak_floor: 0.000001,
    peak_decay: 0.995,
    level_alpha: 0.12,
    band_attack: 0.45,
    band_release: 0.12,
    flux_alpha: 0.25,
    beat_threshold: 0.18,
    beat_cooldown_ms: 0,
    strobe_ms: 40,
    strobe_level: 1,
    strobe_color: null,
  },
  output: {
    change_interval_ms: 120,
    transition_min_ms: 60,
    transition_max_ms: 600,
    transition_beat_weight: 0.75,
    transition_level_weight: 0.25,
    transition_curve: 1,
    deadband_rgb: 3,
    deadband_overall: 0.02,
    brightness_floor: 1,
    gamma_min: 0.1,
    gamma_max: 5,
  },
  spatial: {
    enabled: true,
    layout: "auto",
    room: { min: [-3, 0, -3], max: [3, 2.6, 3] },
    focus: 2,
    omni_floor: 0.15,
    strip_samples: 8,
    height_sharpness: 0.35,
    distance_falloff: 0,
  },
  devices: [
    { type: "shelly", host: "192.168.178.32", device: "rgbw2", min_brightness: 1, max_brightness: 40, brightness_gamma: 1, rgbw_id: 0, auth: null, form: "lamp", position: [-2.2, 1.4, 1.6], extent: [0, 0, 0], path: null, spatiality: 0.8 },
    { type: "govee_lan", ip: "192.168.178.78", name: "Kueche", min_brightness: 10, max_brightness: 100, brightness_gamma: 1, form: "lamp", position: [2.1, 1.1, -1.4], extent: [0, 0, 0], path: null, spatiality: 0.6 },
    { type: "wled", host: "192.168.178.90", leds: 120, segments: 16, protocol: "dnrgb", form: "strip", position: [-2.6, 1.2, 0], extent: [0, 0.9, 0], path: null, spatiality: 1 },
  ] as AppConfig["devices"],
};

const speakers: SpeakerPlacement[] = [
  { channel: 0, role: "front_left", short_name: "FL", direction: [-0.5, 0, 0.866] },
  { channel: 1, role: "front_right", short_name: "FR", direction: [0.5, 0, 0.866] },
  { channel: 2, role: "front_center", short_name: "FC", direction: [0, 0, 1] },
  { channel: 4, role: "side_left", short_name: "SL", direction: [-1, 0, 0] },
  { channel: 5, role: "side_right", short_name: "SR", direction: [1, 0, 0] },
];

const status: EngineStatus = {
  state: "running",
  source: "default output device",
  sample_rate: 48000,
  channels: 6,
  layout: "5.1 (FL FR FC LFE SL SR)",
  speakers,
  devices: ["shelly 192.168.178.32", "govee Kueche", "wled 192.168.178.90"],
  warnings: [],
  error: null,
};

const answers: Record<string, unknown> = {
  load_config: { path: "C:\\mock\\config.json", config, warnings: [], recovered: null, missing: false } satisfies ConfigPayload,
  save_config: [] as string[],
  config_path: "C:\\mock\\config.json",
  set_config_path: "C:\\mock\\config.json",
  engine_status: status,
  engine_start: null,
  engine_stop: null,
  engine_reload: null,
  list_audio_devices: [
    { id: "{mock-0}", name: "CORSAIR VIRTUOSO", layout: "stereo (FL FR)", is_default: true, is_loopback: true },
    { id: "{mock-1}", name: "LG HDR 4K", layout: "5.1 (FL FR FC LFE SL SR)", is_default: false, is_loopback: true },
  ] satisfies AudioDeviceInfo[],
  list_audio_apps: [
    { pid: 1234, exe: "chrome.exe", display: "Google Chrome", playing: true },
    { pid: 4321, exe: "spotify.exe", display: "Spotify", playing: false },
  ] satisfies AppInfo[],
  device_types: [
    { type_tag: "shelly", default_entry: { type: "shelly", host: "192.168.1.50", form: "lamp", position: null, extent: [0, 0, 0], path: null, spatiality: 0 } },
    { type_tag: "govee_lan", default_entry: { type: "govee_lan", ip: "192.168.1.60", form: "lamp", position: null, extent: [0, 0, 0], path: null, spatiality: 0 } },
    { type_tag: "wled", default_entry: { type: "wled", host: "192.168.1.70", segments: 16, form: "lamp", position: null, extent: [0, 0, 0], path: null, spatiality: 0 } },
    { type_tag: "hue", default_entry: { type: "hue", bridge: "192.168.1.80", application_key: "", lights: [], form: "lamp", position: null, extent: [0, 0, 0], path: null, spatiality: 0 } },
  ] as unknown as DeviceTypeInfo[],
  resolve_speakers: speakers,
  test_device: "mock device",
  hue_pair: "mock-application-key",
  hue_lights: [
    { id: "3f1c0b2a-0001", name: "Living room left" },
    { id: "3f1c0b2a-0002", name: "Living room right" },
  ],
  recent_logs: [
    { level: "info", message: "Audio: capturing default output device at 48000 Hz, 5.1 (FL FR FC LFE SL SR)" },
    { level: "warn", message: "Config: band \"treble\" is narrower than one FFT bin, it will read as silent" },
  ] satisfies LogLine[],
  get_prefs: { config_path: null, theme: "system", language: "en" } satisfies AppPrefs,
  set_theme: null,
  set_language: null,
};

export function mockInvoke<T>(command: string): Promise<T> {
  if (!(command in answers)) return Promise.reject({ kind: "engine", message: `No mock for "${command}".` });
  return Promise.resolve(structuredClone(answers[command]) as T);
}
