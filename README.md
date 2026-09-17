# ShellyRGBAudio

A Rust application that synchronizes RGBW lights with your computer's audio output in real time. It captures the Windows audio stream with WASAPI, runs an FFT over it, and maps the result to color and brightness on your lights.

## Features

- **Multi-device:** drive any number of lights at once, of different makes.
- **Shelly:** Gen1 (RGBW2) and Gen2 (Plus RGBW PM), with automatic API detection and optional authentication.
- **Govee LAN:** local UDP control, no cloud account.
- **Fully configurable frequency to color mapping:** you decide which color sits at which frequency; frequencies in between are interpolated.
- **Free frequency bands:** any number, any boundaries, individually weighted and optionally individually colored.
- **Per-app audio:** follow single applications instead of the whole output device, one or more at a time, or everything except a few. See [`audio.apps`](#audioapps).
- **Per-device color:** every light may use its own color map while they all share one audio analysis.
- **Everything else is configurable too:** FFT size and overlap, window function, smoothing, beat detection, strobe, transitions, silence handling, network timeouts.
- **Smart throttling:** deduplicates and rate-limits network traffic so the lights stay responsive.
- **State restoration:** puts the lights back the way it found them on exit.

Windows only for now, because the audio capture uses WASAPI. The capture sits behind one small platform boundary in `src/capture/`, so a Linux or macOS backend can be added there without touching the rest.

## Installation

1. Install [Rust](https://www.rust-lang.org/tools/install).
2. Clone this repository.
3. Build:
   ```bash
   cargo build --release
   ```

## Running

```bash
cargo run --release                   # uses ./config.json
cargo run --release -- my-setup.json  # explicit path
cargo run --release -- --list-devices # print the output devices, then exit
cargo run --release -- --list-apps    # print the applications, then exit
```

The config file can also be pointed at with the `SHELLYRGBAUDIO_CONFIG` environment variable. On the first run, the file is created by default; adjust it and restart.

The config is rewritten on every start, so any option added by a new version shows up in your file automatically, already filled with its default. Your own values are kept. If the file cannot be parsed, a copy is saved next to it as `config.json.bak` before defaults are written.

At startup the program prints which color each band resolved to, so you can check a color map without playing anything:

```
Device: shelly 192.168.178.32
  bass 20-200Hz #FF0000, mid 200-2000Hz #00FF00, treble 2000-8000Hz #0000FF
```

## Frequency to color

This is the heart of the configuration, and it has two layers.

**Bands** split the spectrum into buckets. Each bucket produces one energy value per analysis frame.

**Color stops** define a color axis over frequency. A band that does not name its own `color` looks its color up on that axis, at its center frequency. If no stop sits exactly there, **the two nearest stops are mixed** — which is the point of the whole mechanism. Below the lowest and above the highest stop, the nearest stop's color is held.

The color that actually reaches the light is the energy-weighted mix of all band colors: loud bass pulls the result toward the bass color, loud treble toward the treble color.

```json
{
  "bands": [
    {
      "name": "sub",
      "from_hz": 20,
      "to_hz": 60
    },
    {
      "name": "bass",
      "from_hz": 60,
      "to_hz": 250
    },
    {
      "name": "mid",
      "from_hz": 250,
      "to_hz": 2000,
      "color": "#00FF00"
    },
    {
      "name": "high",
      "from_hz": 2000,
      "to_hz": 6000
    },
    {
      "name": "air",
      "from_hz": 6000,
      "to_hz": 16000,
      "weight": 0.5
    }
  ],
  "color_map": {
    "stops": [
      {
        "hz": 40,
        "color": "#FF0000"
      },
      {
        "hz": 8000,
        "color": "#0000FF"
      }
    ]
  }
}
```

resolves to:

| Band   | Centre  | Color     | Why                                                   |
|--------|---------|-----------|-------------------------------------------------------|
| `sub`  | 34.6 Hz | `#FF0000` | below the first stop, held                            |
| `bass` | 122 Hz  | `#C90036` | **mixed** between the two stops                       |
| `mid`  | 707 Hz  | `#00FF00` | its own `color` beats the stops                       |
| `high` | 3464 Hz | `#2800D7` | **mixed** between the two stops                       |
| `air`  | 9798 Hz | `#0000FF` | above the last stop, held; contributes at half weight |

### colors

colors are written either as a hex string or as an object:

```
"#F00"          "#FF0000"          "#FF0000AA"          { "r": 255, "g": 0, "b": 0, "w": 170 }
```

Three, six, or eight hex digits; the last pair is the white channel. Both forms are accepted everywhere a color is expected, and both are written back as hex.

## Configuration reference

### `audio`

| Setting               | Default              | Description                                                                                                                                                     |
|-----------------------|----------------------|-----------------------------------------------------------------------------------------------------------------------------------------------------------------|
| `device`              | `{"type":"default"}` | Which output to listen to. `{"type":"default"}`, `{"type":"id","id":"..."}` or `{"type":"name","name":"..."}`. An empty name or id lists all devices and exits. |
| `apps`                | see below            | Capture single applications instead of the whole `device`. See [`audio.apps`](#audioapps).                                                                      |
| `fft_size`            | `1024`               | FFT window length in samples. Larger gives finer frequency resolution, smaller reacts faster. Rounded up to a power of two, 64 to 16384.                        |
| `hop_size`            | `1024`               | Samples advanced between two FFTs. Equal to `fft_size` means no overlap; half gives twice as many updates.                                                      |
| `window`              | `"hann"`             | `hann`, `hamming`, `blackman` or `rectangular`.                                                                                                                 |
| `downmix`             | `"average"`          | How a multi-channel frame becomes one sample: `average` (first two), `left`, `right`, `all_channels`.                                                           |
| `buffer_duration_hns` | `200000`             | WASAPI capture buffer in 100 ns units. 200000 is 20 ms.                                                                                                         |
| `silence_timeout_ms`  | `2000`               | No audio for this long counts as silence.                                                                                                                       |
| `silence_fade_ms`     | `800`                | Fade time used when dimming into silence.                                                                                                                       |
| `silence_brightness`  | `0.0`                | Brightness held during silence, 0.0 to 1.0.                                                                                                                     |

### `audio.apps`

Instead of everything that leaves the output device, the lights can follow single applications, the
same ones the Windows volume mixer lists. `--list-apps` prints what can be picked:

```
--- Applications (usable in audio.apps.groups[].apps) ---
  Google Chrome                       chrome.exe               pid 3672     playing
  Steam                               steam.exe                pid 26808    idle
  System sounds (cannot be captured)  -                        pid 0        idle
```

A pattern matches the display name (`"Google Chrome"`) or the executable (`chrome.exe`, `chrome`),
case insensitively. All matched apps are mixed into one signal, so the lights keep reacting to one
analysis; a group exists to organize the patterns and to give them their own `gain`.

| Setting                | Default     | Description                                                                                                               |
|------------------------|-------------|---------------------------------------------------------------------------------------------------------------------------|
| `enabled`              | `false`     | `false` captures `audio.device`, exactly as before.                                                                       |
| `mode`                 | `"include"` | `include` follows the listed apps, `exclude` follows everything except them.                                              |
| `include_process_tree` | `true`      | Also capture the child processes of a matched app, even when their executable is named differently.                       |
| `rescan_ms`            | `3000`      | How often the running applications are rechecked, so an app that starts or restarts later is picked up.                   |
| `sample_rate`          | `48000`     | Capture format. Per-app capture cannot ask the system for a mix format, so it has to be stated. Windows converts for you. |
| `channels`             | `2`         | Same, for the channel count.                                                                                              |
| `groups`               | `[]`        | The app groups, see below.                                                                                                |

Each group takes a `name` (free text, and reserved for routing a group to one light later), an
`enabled` flag, the `apps` patterns, and a `gain` that scales that group in the mix.

```json
{
  "audio": {
    "apps": {
      "enabled": true,
      "mode": "include",
      "groups": [
        { "name": "Media", "enabled": true, "apps": ["Google Chrome", "spotify.exe"], "gain": 1.0 },
        { "name": "Game", "enabled": false, "apps": ["cs2.exe"], "gain": 0.8 }
      ]
    }
  }
}
```

While none of the selected apps plays anything, that counts as silence: the lights dim to
`silence_brightness` after `silence_timeout_ms`, the same as with a quiet output device.

Good to know:

* Needs Windows 10 version 2004 or newer, which is where per-process loopback capture was added.
* "System sounds" cannot be captured. It has no process to attach to.
* A loopback client only receives streams that an app opens after it was attached to, so the app
  is attached to as soon as its process exists rather than when it becomes audible. In `exclude`
  mode only audible apps can be found, so an app already playing when the program started
  joins in when it next starts a sound.
* `exclude` mode captures every other app separately rather than using the Windows exclude mode,
  because that one takes a single process and two of them would capture everything twice.

### `bands`

An array. Order does not matter, it gets sorted by `from_hz`. Bands may overlap, in which case the shared frequencies count toward both.

| Setting             | Default        | Description                                                                       |
|---------------------|----------------|-----------------------------------------------------------------------------------|
| `name`              | `"band"`       | Used in log output and to address the band from a per-device override.            |
| `from_hz` / `to_hz` | `20` / `20000` | Boundaries. A band with `from_hz >= to_hz` is dropped with a warning.             |
| `weight`            | `1.0`          | How strongly this band pulls the mixed color and the overall level. `0` mutes it. |
| `color`             | `null`         | Fixed color. `null` means: look it up on `color_map.stops`.                       |

### `color_map`

| Setting                      | Default                                   | Description                                                                                                                                                                                                                                                                                                                                       |
|------------------------------|-------------------------------------------|---------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------|
| `stops`                      | red / green / blue at 63, 632 and 4000 Hz | The color axis. Sorted automatically; stops at `hz <= 0` are dropped.                                                                                                                                                                                                                                                                             |
| `frequency_scale`            | `"log"`                                   | `log` spaces the stops the way frequency is heard — 1000 Hz sits near the middle between 400 and 6000 Hz. `linear` spaces them arithmetically. Also decides whether a band's centre is its geometric or arithmetic middle.                                                                                                                        |
| `interpolation`              | `"srgb"`                                  | `srgb` blends the channel values directly. `linear_light` blends in linear light, so midpoints stay bright. `hsv` travels along the color wheel.                                                                                                                                                                                                  |
| `saturation`                 | `1.0`                                     | How far the mixed color is pushed back to full saturation, 0.0 to 1.0. Band energies normally sit fairly close together, so the plain mix lands near grey and grey carries no hue. Stretching the span between the weakest and the strongest channel across the full range turns those differences back into real color. `0.0` sends the raw mix. |
| `value_floor` / `value_span` | `0.15` / `0.85`                           | color brightness is `floor + span * level`, applied before each device's own brightness curve.                                                                                                                                                                                                                                                    |
| `white_channel`              | `"off"`                                   | `off` leaves white dark. `min_channel` moves the common part of R, G and B into the white channel, which gives a cleaner white on real RGBW strips. `from_color` keeps whatever white the colors carried. `fixed` always drives `white_fixed`.                                                                                                    |
| `white_fixed`                | `0`                                       | White level for `white_channel: "fixed"`.                                                                                                                                                                                                                                                                                                         |
| `fallback_color`             | `"#FF0000"`                               | Shown when nothing is playing, or when neither a stop nor a band color can answer.                                                                                                                                                                                                                                                                |

### `dynamics`

| Setting            | Default    | Description                                                                                                                                                                                                                                                                                                                                                                                                                          |
|--------------------|------------|--------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------|
| `normalize`        | `"shared"` | What each band's energy is measured against. `shared` uses one reference level for all bands, so relative loudness between them survives and the color actually follows the music. `per_band` gives every band its own rolling peak; each band then uses its full range alone, but all cross-band contrast is lost — a steady signal drives every band to 1.0 no matter how quiet it is next to the others, and the color collapses. |
| `log_offset`       | `1.0`      | Offset in the `ln(power + offset)` loudness compression. Larger flattens quiet passages.                                                                                                                                                                                                                                                                                                                                             |
| `peak_floor`       | `1e-6`     | Lower bound of the per-band peak tracker. Acts as the noise floor.                                                                                                                                                                                                                                                                                                                                                                   |
| `peak_decay`       | `0.995`    | Per-frame decay of the peak tracker. Closer to 1 adapts more slowly to changing loudness.                                                                                                                                                                                                                                                                                                                                            |
| `level_source`     | `"peak"`   | Where brightness comes from. `peak` uses the loudest band, so brightness follows how loud the music is whatever part of the spectrum carries it. `average` uses the mean across all bands, which measures how *wide* the spectrum is as much as how loud — music living in a single band, bass especially, then comes out dim.                                                                                                       |
| `level_alpha`      | `0.12`     | Smoothing of the overall level. Larger reacts faster.                                                                                                                                                                                                                                                                                                                                                                                |
| `band_attack`      | `0.45`     | How fast a band's energy may **rise**. This decides how quickly the color answers the music, so keep it high. `1.0` follows every frame instantly.                                                                                                                                                                                                                                                                                   |
| `band_release`     | `0.12`     | How fast a band's energy may **fall**. Lower values make the color glide instead of flicker, and cost no responsiveness because rises go through `band_attack`. Beat detection uses the unsmoothed values either way, so neither knob dulls the strobe.                                                                                                                                                                              |
| `flux_alpha`       | `0.25`     | Smoothing of the spectral flux used for beat detection.                                                                                                                                                                                                                                                                                                                                                                              |
| `beat_threshold`   | `0.18`     | Flux above this counts as a beat. Lower means more beats.                                                                                                                                                                                                                                                                                                                                                                            |
| `beat_cooldown_ms` | `0`        | Minimum time between two beats. `0` lets every frame above the threshold retrigger.                                                                                                                                                                                                                                                                                                                                                  |
| `strobe_ms`        | `40`       | How long a beat holds the strobe.                                                                                                                                                                                                                                                                                                                                                                                                    |
| `strobe_level`     | `1.0`      | Brightness during a strobe.                                                                                                                                                                                                                                                                                                                                                                                                          |
| `strobe_color`     | `null`     | color forced during a strobe. `null` keeps the music color.                                                                                                                                                                                                                                                                                                                                                                          |

### `output`

| Setting                                              | Default         | Description                                                                                      |
|------------------------------------------------------|-----------------|--------------------------------------------------------------------------------------------------|
| `change_interval_ms`                                 | `120`           | Minimum time between two commands per light. Lower reacts faster but stresses the device.        |
| `transition_min_ms` / `transition_max_ms`            | `60` / `600`    | Fade time range. Louder and beatier music fades faster.                                          |
| `transition_beat_weight` / `transition_level_weight` | `0.75` / `0.25` | How much beat strength and overall level each shorten the fade.                                  |
| `transition_curve`                                   | `1.0`           | Shapes the fade ramp. `1.0` is linear; above 1 keeps fades long until the music really picks up. |
| `deadband_rgb`                                       | `3`             | A frame is only sent if a color channel moved at least this much.                                |
| `deadband_overall`                                   | `0.02`          | ... or the level moved at least this much.                                                       |
| `brightness_floor`                                   | `1`             | Lowest brightness ever sent. `1` keeps lights from switching off entirely.                       |
| `gamma_min` / `gamma_max`                            | `0.1` / `5.0`   | Bounds each device's `brightness_gamma` is clamped into.                                         |

### `devices`

An array of light definitions, each identified by its `type`.

Common to every type:

| Setting            | Default   | Description                                                                                                               |
|--------------------|-----------|---------------------------------------------------------------------------------------------------------------------------|
| `min_brightness`   | `1`       | Lower brightness limit, 0 to 100.                                                                                         |
| `max_brightness`   | `80`      | Upper brightness limit, 1 to 100.                                                                                         |
| `brightness_gamma` | `0.6`     | Brightness curve. Below 1 lifts quiet passages, above 1 emphasises peaks.                                                 |
| `color_map`        | inherited | Optional. Overrides the global color map for this light only; keys you leave out are inherited.                           |
| `bands`            | inherited | Optional. Overrides `color` and `weight` of the global bands, matched by `name`. Band boundaries stay global — see below. |

#### `type: "shelly"`

| Setting                  | Default          | Description                                                                         |
|--------------------------|------------------|-------------------------------------------------------------------------------------|
| `host`                   | `192.168.178.50` | IP address or hostname.                                                             |
| `device`                 | `"auto"`         | `auto`, `rgbw2` (Gen1) or `plus_rgbw_pm` (Gen2).                                    |
| `rgbw_id`                | `0`              | Component ID, relevant for Gen2.                                                    |
| `auth`                   | `null`           | `{"username":"...","password":"..."}` for protected devices.                        |
| `http_timeout_ms`        | `3000`           | Per-request timeout.                                                                |
| `gen2_min_transition_ms` | `500`            | Gen2 refuses very short fades, so anything below this is sent without a transition. |
| `gen2_max_transition_s`  | `10600`          | Upper cap Gen2 accepts.                                                             |

#### `type: "govee_lan"`

| Setting                          | Default            | Description                                    |
|----------------------------------|--------------------|------------------------------------------------|
| `ip`                             | `192.168.1.60`     | IP address of the light.                       |
| `name`                           | `"H6008"`          | Label used in log output.                      |
| `remote_port`                    | `4003`             | Port on the light.                             |
| `color_temp_kelvin`              | `0`                | `0` keeps the light in RGB mode.               |
| `local_bind_addr` / `local_port` | `0.0.0.0` / `4002` | Local socket. The protocol requires port 4002. |
| `read_timeout_ms`                | `300`              | How long to wait for a status reply.           |

> All Govee lights in one process share a single UDP socket, because the protocol pins the local port. `local_bind_addr`, `local_port` and `read_timeout_ms` therefore come from the first Govee entry; a second entry asking for different values gets a warning and the existing socket. `remote_port` is per light.

### Per-device color

Each light may override the color mapping while sharing the same audio analysis:

```json
{
  "type": "govee_lan",
  "ip": "192.168.178.78",
  "name": "Kitchen",
  "color_map": {
    "stops": [
      { "hz": 60,   "color": "#FF00FF" },
      { "hz": 5000, "color": "#00FFFF" }
    ]
  },
  "bands": [
    { "name": "treble", "weight": 0.4 }
  ]
}
```

Keys left out of a device's `color_map` are inherited from the global section, so the snippet above changes only the stops. A device's `bands` entries may change `color` and `weight`, matched by `name`; `from_hz` and `to_hz` are ignored with a warning, because the band energies are computed once for everyone and the boundaries have to stay identical.

## Upgrading from an older config

Older versions kept `change_interval_ms`, `audio_device`, `transition_min_ms`, `transition_max_ms`, `beat_threshold` and `strobe_ms` at the top level, and lights in `shellys[]` / `govees[]`. Both layouts are migrated automatically on the first start: your values move into the new sections, the old keys disappear, and the new sections appear with their defaults. Nothing needs to be edited by hand.

**Two behavioral changes:**

- Colors used to come from a fixed hue calculation over exactly three bands. They now come from the band and stop mapping described above. With the default configuration the assignment is the same — bass red, mids green, treble blue — but transitions are smoother.
- Band energies are now measured against one shared reference level (`dynamics.normalize: "shared"`). The old per-band normalization gave every band its own rolling peak, which meant a sustained sound drove all bands to their maximum at once regardless of how loud they really were relative to each other; the resulting mix carried no hue. Set `dynamics.normalize` to `per_band` to get the old measurement back.
- Brightness comes from the loudest band (`dynamics.level_source: "peak"`) instead of the mean across bands. With the shared reference level above, the mean would read a bass-only passage — one full band, the rest near zero — as quiet and dim as the lights. Set `level_source` to `average` for the mean.

The `audio.apps` block is new and appears with `enabled: false`, so an existing config keeps capturing the output device until you turn it on.

## Troubleshooting

- **No audio detected:** check that `audio.device` points at the output you actually play through. Run with `--list-devices` to have every device listed.
- **A selected app does not drive the lights:** run `--list-apps` while it plays and use the name it prints there. An app is only found once its process runs, so give it up to `audio.apps.rescan_ms`. "System sounds" cannot be captured at all.
- **App capture fails on start:** per-process capture needs Windows 10 version 2004 or newer. Older builds only support `audio.device`.
- **Light is not responding:** check `host` / `ip` and that your computer can reach it. A device that cannot be reached at startup aborts the program with the underlying error.
- **Latency:** lower `output.change_interval_ms`, or lower `audio.hop_size` for more frequent analysis.
- **Everything comes out white or gray:** the bands are reading nearly the same level. Check that `dynamics.normalize` is `shared` and raise `color_map.saturation` toward `1.0`. Giving the bands more distinct colors, or fewer and wider bands, helps too.
- **colors jump instead of gliding:** lower `dynamics.band_release` first — it damps the falling side without slowing the response. Then check `output.transition_min_ms`: at `0` the lights snap instantly during loud passages, which is very visible once the colors are saturated. Lowering `output.change_interval_ms` helps too, since it decides how many steps a transition is made of. `dynamics.beat_cooldown_ms` calms a strobe that retriggers constantly.
- **Reaction feels sluggish:** raise `dynamics.band_attack` toward `1.0`. Do not lower `band_release` to compensate for something else — only the attack governs how fast the color arrives. `output.change_interval_ms` is the hard ceiling: at 360 ms nothing can update more than about three times a second.
- **Bass-heavy music stays dark:** check that `dynamics.level_source` is `peak`. On `average`, brightness partly measures how wide the spectrum is, and bass fills only one band.
- **Shelly crashes or stutters:** raise `output.change_interval_ms` or lower `max_brightness`.
- **Authentication issues:** make sure `auth` is set for password-protected Shelly devices.
- **Config keeps resetting:** the file failed to parse; look for `config.json.bak` next to it and check stderr for the exact line and column.
