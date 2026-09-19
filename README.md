# ShellyRGBAudio

A Rust application that synchronizes RGBW lights with your computer's audio output in real time. It captures the system audio, runs an FFT over it, and maps the result to color and brightness on your lights. Runs on Windows, Linux and macOS.

## Features

- **Multi-device:** drive any number of lights at once, of different makes.
- **Shelly:** Gen1 (RGBW2) and Gen2 (Plus RGBW PM), with automatic API detection and optional authentication.
- **Govee LAN:** local UDP control, no cloud account, with an optional DreamView mode that lifts the light's rate limiting.
- **WLED:** driven over the UDP realtime protocol, with a real gradient along the strip.
- **Philips Hue:** local control over the bridge's CLIP v2 API, paired from the settings window. Several lights in one entry act as the segments of one strip.
- **Addressable strips:** a positioned strip is colored per segment, so bass can sit at one end and treble at the other instead of the whole strip showing one color.
- **Fully configurable frequency to color mapping:** you decide which color sits at which frequency; frequencies in between are interpolated.
- **Free frequency bands:** any number, any boundaries, individually weighted and optionally individually colored.
- **Per-app audio:** follow single applications instead of the whole output device, one or more at a time, or everything except a few. See [`audio.apps`](#audioapps).
- **Per-device color:** every light may use its own color map while they all share one audio analysis.
- **3D positioning:** place each light in the room and it follows the speakers it faces, so a left light answers the left channel and a rear light the surrounds. Lights are either a **lamp** (a point) or a **light strip**, which may run straight or bend around corners. See [`spatial`](#spatial).
- **Everything else is configurable too:** FFT size and overlap, window function, smoothing, beat detection, strobe, transitions, silence handling, network timeouts.
- **Smart throttling:** deduplicates and rate-limits network traffic so the lights stay responsive.
- **State restoration:** puts the lights back the way it found them on exit.
- **Tray application:** lives in the notification area with a status line, pause, reload and quit, and a settings window that edits every option below without touching JSON by hand.

Runs on Windows, Linux and macOS. Everything except the audio capture is platform independent; capture sits behind one small boundary in `src-tauri/src/capture/`, with a WASAPI backend on Windows and a [cpal](https://github.com/RustAudio/cpal) one on Linux and macOS.

## Layout

The standard [Tauri](https://v2.tauri.app) layout: the frontend at the root, the Rust in `src-tauri/`.

| Path | What it is |
|---|---|
| `src/` | The settings window — React and TypeScript, built by Vite. Run `pnpm dev` on its own and it opens in an ordinary browser against the mock answers in `src/devMocks.ts`, which is the quickest way to work on the interface. |
| `src/locales/` | The interface text. `en.json` is the base — every key lives there, and a translation that has not caught up falls back to it. German is `de_DE.json`. |
| `src/types/` | TypeScript declarations generated from the Rust config structs. Regenerate with `pnpm types` after changing any of them. |
| `src-tauri/src/` | The application: `lib.rs`, `commands.rs`, `tray.rs`, `paths.rs`. |
| `src-tauri/src/engine.rs` + `capture/`, `devices/`, `analysis.rs`, `color.rs`, `config.rs`, `spatial.rs` | The engine. Knows nothing about Tauri and runs entirely on threads `Engine` owns, which is what keeps the pipeline off the thread the window's event loop needs. |
| `src-tauri/windows/installer.nsi` | A fork of Tauri's NSIS script, adding the page that asks where `config.json` should live. |

## Installation

1. Install [Rust](https://www.rust-lang.org/tools/install) and [Node](https://nodejs.org) with [pnpm](https://pnpm.io).
2. Clone this repository.
3. Install the build dependencies for your platform:
   - **Windows** — none. WebView2 ships with Windows 11.
   - **Linux** — the ALSA headers, which cpal needs to build: `sudo apt install libasound2-dev` (Debian/Ubuntu) or `sudo dnf install alsa-lib-devel` (Fedora), plus the [Tauri prerequisites](https://v2.tauri.app/start/prerequisites/).
   - **macOS** — Xcode command line tools, and see [Capturing on macOS](#capturing-on-macos) before running.
4. Build:
   ```bash
   pnpm install
   pnpm tauri build     # installer in target/release/bundle/
   ```

   Or, to run it without packaging:
   ```bash
   pnpm tauri dev
   ```

### Capturing the system audio

Windows can record an output device directly. Linux and macOS cannot: what they offer is a *capture* device that happens to carry the output, and the two platforms differ in where it comes from.

The **Audio** tab lists what is available; on Linux and macOS the usable ones are the monitor or virtual devices described below.

#### Capturing on Linux

PulseAudio and PipeWire give every output a matching **monitor source**, and that is what gets recorded. Nothing has to be set up: leave `audio.device` on its default and the monitor is found automatically.

The device dropdown looks roughly like this; the `.monitor` entry is the one that carries the output:

```
alsa_input.pci-0000_00_1f.3.analog-stereo            (Standard)
alsa_output.pci-0000_00_1f.3.analog-stereo.monitor
```

PipeWire works through its `pipewire-pulse` compatibility layer, which every desktop that ships PipeWire also ships. On a machine running neither, capture falls back to ALSA, which can only see real recording hardware — there will be no `[system output]` entry, and a loopback device has to be set up by hand with `snd-aloop`.

#### Capturing on macOS

macOS has no loopback of its own, so it needs a **virtual audio driver**. [BlackHole](https://existential.audio/blackhole) is the usual free one; Loopback and Soundflower work too.

1. Install BlackHole.
2. Open **Audio MIDI Setup** → **+** → **Create Multi-Output Device**, and tick both your real speakers and BlackHole.
3. Select that Multi-Output Device as the system output. The sound goes to both, so you still hear it.
4. Leave `audio.device` on its default — BlackHole is recognised automatically — or name it explicitly.

macOS asks for microphone permission the first time the program records; it has to be granted, because a virtual audio device is a recording device as far as the system is concerned.

> **Per-application capture is Windows only.** [`audio.apps`](#audioapps) relies on process loopback, which has no equivalent through cpal. Turning it on elsewhere prints a warning and records the whole output device instead.
>
> **Speaker layouts are guessed off Linux and macOS.** WASAPI reports which speaker sits on which channel; cpal does not, so the layout comes from the channel count alone (2 → stereo, 6 → 5.1, 8 → 7.1). That is right for almost every setup, and [`spatial.layout`](#spatial) states it by hand where it is not.

## Running

The application starts into the notification area and begins capturing straight away — no window opens until you ask for one.

**The tray menu**

| Entry | What it does |
|---|---|
| *(status line)* | What the engine is doing: sample rate, speaker layout, number of devices. Not clickable. |
| **Pause** / **Resume** | Stops the pipeline and puts the lights back where they were; resuming builds everything again from the file. |
| **Settings…** | Opens the settings window. A left click on the tray icon does the same. |
| **Reload** | Restores the lights, then rebuilds capture, analysis and devices from the config on disk. |
| **Quit** | Restores the lights and exits. |

Closing the settings window only hides it; the engine keeps running. **Quit** is the way out.

**The settings window** edits everything documented under [Configuration reference](#configuration-reference), with the audio device and the running applications read live from the system. Saving writes the file; *Save & reload* also restarts the engine so the change takes effect immediately.

**Language.** English by default, German available, and "follow the system" picks between them from the OS locale. The setting is under **Advanced** and covers the window and the tray menu; the engine's log stays English, because it is diagnostic output that ends up in bug reports and is more useful when everyone's copy reads the same. Adding a language means one more file in `src/locales/`, a line in `LANGUAGES` in `src/i18n.ts`, and — if the tray should speak it too — a column in `src-tauri/src/i18n.rs`.

**Placing the lights** happens under **Room**. The room view draws the room, the speakers the engine is actually weighting against, and every positioned light; dragging the handle moves one, and the *Drehen* mode turns a strip. A strip is stated as a **length** and two angles rather than the half-vector the config stores — 1.4 metres running up the wall, not `[0, 0.7, 0]` — and the two representations are kept in step, so typing a length and dragging in the view are the same edit seen from two sides. **Around the corner** turns a straight strip into a corner list ([`path`](#strips-that-bend)): every corner then gets a ball of its own in the view that can be dragged where the strip really turns, the marker in the middle moves the whole strip, and *Begradigen* in the form takes it back to a straight line. A bent strip has one direction per run rather than one overall, so *Drehen* works per run: click the run you mean and the strip hinges at the corner it starts from, carrying everything after it along. The form states the same thing as a length and two angles per run.

### Where the config lives

The first of these that answers wins:

1. `--config <path>` on the command line
2. the `SHELLYRGBAUDIO_CONFIG` environment variable
3. `config_path` in `%APPDATA%\eu.sytxlabs.shellyrgbaudio\app.json` — written by the installer's location page, and by the **Advanced** tab
4. a `config.json` next to the executable, for a portable copy
5. `%APPDATA%\eu.sytxlabs.shellyrgbaudio\config.json`

The installer asks for the location during setup, and `ShellyRGBAudio_x64-setup.exe /S /CONFIGPATH=D:\somewhere\config.json` sets it for an unattended install.

Reading the config migrates older layouts forward and fills in any option a new version added, but nothing is written back until you save. A file that cannot be parsed is copied to `config.json.bak` and the window says so; **the original is left alone**, so a single mistyped value cannot cost you the file.

At startup the engine logs which colour each band resolved to, visible under **Status**, so a colour map can be checked without playing anything:

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
same ones the Windows volume mixer lists. the **Applications** tab lists what can be picked:

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
| `sample_rate`          | `null`      | Capture format. Per-app capture has no mix format of its own to ask for, so `null` takes the output device's rate. A number states one instead and Windows converts for you. |
| `channels`             | `null`      | Same, for the channel count: `null` records in as many channels as the output device runs in, with its speaker mask, so a 7.1 output stays 7.1 and [3D positioning](#3d-positioning) keeps working while capturing single apps. A number pins it — `2` on a 7.1 output is what makes the status line read `stereo (FL FR)`, and startup says so. |
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

### `spatial`

Places the lights in the room so each one follows the part of the audio that comes from its direction. Off by default; see [3D positioning](#3d-positioning) for the whole picture.

| Setting            | Default                                 | Description                                                                                                                       |
|--------------------|-----------------------------------------|-----------------------------------------------------------------------------------------------------------------------------------|
| `enabled`          | `false`                                 | While false no per-channel analysis runs at all and every light follows the global mix, exactly as before.                         |
| `layout`           | `"auto"`                                | `"auto"` reads the speaker layout from the audio device. To state it by hand: `{"channels": ["front_left", "front_right", ...]}`. |
| `room`             | `{"min":[-3,0,-3],"max":[3,3,3]}`       | Bounds of the room in metres. Only `min.y` / `max.y` are used today, to map a light's height onto the spectrum.                   |
| `focus`            | `2.0`                                   | How tightly a light aims at the speakers it faces. Higher separates the lights more sharply.                                       |
| `omni_floor`       | `0.15`                                  | Share of the audio every light hears regardless of direction, so a hard left light never loses the right channel entirely.        |
| `strip_samples`    | `8`                                     | How many points a `strip` light is sampled at along its length.                                                                   |
| `height_sharpness` | `0.35`                                  | Width of the height-to-frequency match. Smaller ties a height to fewer bands.                                                      |
| `distance_falloff` | `0.0`                                   | Brightness lost per metre from the listener. `0.0` keeps every light equally bright.                                              |

### `devices`

An array of light definitions, each identified by its `type`.

Common to every type:

| Setting            | Default    | Description                                                                                                               |
|--------------------|------------|---------------------------------------------------------------------------------------------------------------------------|
| `min_brightness`   | `1`        | Lower brightness limit, 0 to 100.                                                                                         |
| `max_brightness`   | `80`       | Upper brightness limit, 1 to 100.                                                                                         |
| `brightness_gamma` | `0.6`      | Brightness curve. Below 1 lifts quiet passages, above 1 emphasises peaks.                                                 |
| `color_map`        | inherited  | Optional. Overrides the global color map for this light only; keys you leave out are inherited.                           |
| `bands`            | inherited  | Optional. Overrides `color` and `weight` of the global bands, matched by `name`. Band boundaries stay global — see below. |
| `form`             | `"lamp"`   | `"lamp"` is a point, `"strip"` runs along a line or a `path`. See [3D positioning](#3d-positioning).                                          |
| `position`         | `null`     | Where the light is, in metres. A lamp sits here; a strip is centred here. `null` leaves it unpositioned — see below.      |
| `extent`           | `[0,0,0]`  | Straight strips only. Half the strip's length as a vector: it runs from `position - extent` to `position + extent`.       |
| `path`             | `null`     | The corners a strip actually follows, for one that bends. Overrides `position` and `extent`. See [strips that bend](#strips-that-bend). |
| `spatiality`       | `0.0`      | How much position overrides the global mix. `0.0` ignores it entirely, `1.0` is fully directional.                        |
| `elevation_tilt`   | `null`     | How strongly height maps onto frequency. `null` decides automatically, see below.                                         |
| `focus`            | inherited  | Optional. Overrides `spatial.focus` for this light.                                                                       |
| `distance_falloff` | inherited  | Optional. Overrides `spatial.distance_falloff` for this light.                                                            |

> **A light is only positioned once it has a `position` or a `path`.** Until then every other field in this block is inert and the light behaves exactly as it did before positions existed: it follows the global mix and shows one color, addressable or not. Setting `spatiality` without placing the light says so at startup rather than guessing where it is.

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
| `dreamview`                      | `false`            | Puts the light into Razer / DreamView mode for the run, which lifts the rate limiting it normally applies to color changes. Colors still go out as the ordinary documented commands, so a light that ignores the mode behaves exactly as before. |

> All Govee lights in one process share a single UDP socket, because the protocol pins the local port. `local_bind_addr`, `local_port` and `read_timeout_ms` therefore come from the first Govee entry; a second entry asking for different values gets a warning and the existing socket. `remote_port` is per light.

#### `type: "wled"`

Driven over WLED's UDP realtime protocol on port 21324, which is low latency and needs no HTTP request per frame. **Addressable:** give the light a position and each segment is colored from where that segment physically is.

| Setting              | Default        | Description                                                                                                                             |
|----------------------|----------------|-------------------------------------------------------------------------------------------------------------------------------------------|
| `host`               | `192.168.1.70` | IP address or hostname.                                                                                                                 |
| `leds`               | `null`         | Pixel count. `null` reads it from the controller over HTTP; state it by hand if that request cannot get through.                        |
| `segments`           | `16`           | How many independently colored runs the pixels are divided into. `1` makes the whole strip one color.                                   |
| `protocol`           | `"dnrgb"`      | `warls`, `drgb`, `drgbw` or `dnrgb`. Only `dnrgb` can address past pixel 490, so it is the default. Use `drgbw` for an RGBW strip.      |
| `port`               | `21324`        | WLED's realtime port.                                                                                                                   |
| `realtime_timeout_s` | `2`            | WLED resumes its own effect this long after the last packet. This is also what restores the strip when the program exits.               |
| `reverse`            | `false`        | Flips which end of the strip segment 0 sits at, for a strip mounted the other way round.                                                |
| `http_timeout_ms`    | `3000`         | Timeout for the two HTTP requests, made only at startup and shutdown.                                                                   |
| `local_bind_addr`    | `0.0.0.0:0`    | Local UDP socket. The default lets the OS pick a port.                                                                                  |

> The realtime protocol carries no brightness field, so `min_brightness` / `max_brightness` / `brightness_gamma` are multiplied into the pixel values before sending. Nothing has to be restored by hand: the `realtime_timeout_s` countdown hands the strip back to whatever effect it was running, and only the brightness and power WLED reported at startup are written back.

#### `type: "hue"`

Philips Hue over the bridge's local CLIP v2 API. No cloud account: the bridge is talked to directly, and pairing is a button press on the bridge itself. **Addressable:** name several lights in one entry and they act as the segments of one strip, colored in the order they are listed.

| Setting                 | Default          | Description                                                                                                      |
|-------------------------|------------------|------------------------------------------------------------------------------------------------------------------|
| `bridge`                | `192.168.1.80`   | IP address or hostname of the bridge.                                                                            |
| `application_key`       | `""`             | What the bridge hands out when pairing. The **Pair** button in the settings window fills this in.                 |
| `lights`                | `[]`             | Resource ids of the `light` resources to drive, in the order they stand in the room. The settings window lists them by name. |
| `max_updates_per_second`| `10`             | The bridge forwards roughly ten commands a second per light. Frames that arrive faster are skipped; the newest one is sent. |
| `transitions`           | `true`           | Passes the fade the engine asks for on to the light. `false` makes every change a jump, which follows the beat more closely. |
| `http_timeout_ms`       | `2000`           | Per-request timeout.                                                                                             |
| `verify_tls`            | `false`          | A bridge signs its certificate itself, so verification is off by default. Turn it on only if the bridge carries a certificate your system trusts. |

Pairing: press the round button on the bridge, then **Pair** in the device's form within half a minute. The key stays valid, so this is done once per bridge.

> A Hue light is told a chromaticity plus a brightness rather than an RGB triple, so the bright-dark part of a color is sent as `dimming` instead of being folded into the color. Frames are sent from a thread of their own, so a slow or unreachable bridge never holds up the other lights. State is restored on exit: on/off, brightness and the color or colour temperature each light had at startup.

#### `type: "govee_dreamview"`

A Govee strip driven in DreamView / Razer mode, with an attempt at addressing its segments individually.

| Setting     | Default          | Description                                                                                                       |
|-------------|------------------|---------------------------------------------------------------------------------------------------------------------|
| `ip`        | `192.168.1.61`   | IP address of the light.                                                                                          |
| `name`      | `"DreamView"`    | Label used in log output.                                                                                         |
| `segments`  | `1`              | How many segments the strip exposes, at most 15. `1` uses only the documented whole-device commands — see below.  |
| `reverse`   | `false`          | Flips which end of the strip segment 0 sits at.                                                                   |

Everything else (`remote_port`, `local_bind_addr`, `local_port`, `read_timeout_ms`, `color_temp_kelvin`) works exactly as in [`govee_lan`](#type-govee_lan), and the same shared-socket note applies.

> **What is verified and what is not.** DreamView / Razer mode itself works: the mode packet is the one the Govee apps send, and the colors go out as the ordinary documented `colorwc` and `brightness` commands. That is what `segments: 1` does, and it is the setting to use if you want something dependable.
>
> **Per-segment addressing is not verified against hardware.** Govee's published LAN API has no segment command at all — segment control is a BLE feature on the models that have it. This device sends the BLE segment command through the LAN Razer transport, which is the most plausible mapping but may simply be ignored. Setting `segments` above 1 prints a reminder at startup. If the strip does not follow the gradient, set `segments: 1` and you are back on the path that works.

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

## 3D positioning

Normally every light shows the same color at the same moment. Give them positions and they stop agreeing: each one follows the speakers it faces, so a light on the left of the desk answers the left channel and a light behind the couch answers the surrounds.

### The room

The listener sits at the origin, distances are metres, and the axes are:

```
        +Y up
         |
         |      +Z front (toward the screen/speakers)
         |     /
         |    /
  -X ----+---/---- +X right
        /
      -Z behind
```

So `[-2.0, 1.2, 0.5]` is two metres to the left, 1.2 up, half a metre in front of you.

### Getting started

1. Open the **Audio** tab and look at the layout shown next to the output you actually use:

   ```
   - Kopfhörer (CORSAIR VIRTUOSO Wireless Gaming Headset)
     id: {0.0.0.00000000}.{b77aed6a-8806-4872-a38b-5d52500a3b1f}
     layout: 7.1 (FL FR FC LFE BL BR SL SR)
   ```

   `stereo` gives you a left/right axis. `5.1` or `7.1` gives you front/back as well.

2. Set `spatial.enabled` to `true`.
3. Give each light a `position` (or a [`path`](#strips-that-bend)) and raise its `spatiality`. Nothing changes until you do: a light with neither keeps the behaviour it had before positions existed.

```json
{
  "spatial": { "enabled": true },
  "devices": [
    {
      "type": "shelly", "host": "192.168.178.32",
      "form": "strip",
      "position": [-2.0, 1.2, 0.5],
      "extent":   [ 0.0, 1.0, 0.0],
      "spatiality": 0.8
    },
    {
      "type": "govee_lan", "ip": "192.168.178.78", "name": "Kitchen",
      "form": "lamp",
      "position": [2.2, 1.6, -1.0],
      "spatiality": 1.0
    }
  ]
}
```

At startup each positioned light prints what it ended up hearing:

```
Device: shelly 192.168.178.32
  bass 20-200Hz #FF0000, mid 200-2000Hz #00FF00, treble 2000-8000Hz #0000FF
  strip [-2.0, 0.2, 0.5] -> [-2.0, 2.2, 0.5] x0.80, FL 44% SL 21% BL 14%
```

### Lamps and strips

A `lamp` is a single point. A `strip` is a line: it is sampled at `spatial.strip_samples` points along its length, and those samples are averaged. A straight strip is written as a centre plus a half-vector (`position` + `extent`); one that bends uses [`path`](#strips-that-bend) instead.

That one difference is all there is, and everything else follows from it. A strip covers a region of the room instead of a spot, so it reacts more widely and more smoothly than a lamp in the same place. A floor-to-ceiling strip also spans every height at once, which — see below — means it spans the whole spectrum and ends up close to the global mix, while a lamp at either end of it leans hard into bass or treble.

### Strips that bend

Real strips rarely run in a straight line. One might go up the wall from the floor, turn at the ceiling, and carry on toward the back of the room. Write that as `path` — every corner it passes through, in order:

```json
{
  "type": "wled", "host": "192.168.178.90",
  "leds": 120, "segments": 24,
  "form": "strip",
  "path": [
    [-2.0, 0.0,  1.5],
    [-2.0, 2.4,  1.5],
    [ 2.0, 2.4, -2.5]
  ],
  "spatiality": 1.0
}
```

That is: start on the floor two metres to your left and slightly in front, run 2.4 m straight up the wall, then turn and run along the ceiling to the back right of the room.

Segments are spread along the path **by distance, not by corner**, because that is how the LEDs are spaced. In the example the wall leg is 2.4 m and the ceiling leg 5.7 m, so the ceiling gets a bit over twice as many segments as the wall. Startup prints the shape back so you can check it:

```
strip [-2.0, 0.0, 1.5] -> [-2.0, 2.4, 1.5] -> [2.0, 2.4, -2.5] (8.1m over 2 legs) x1.00, BR 22% FL 20% SL 19%, 12 segment gradient
```

The settings window writes this for you: pick the strip under **Room**, press **Around the corner**, and drag the corner balls where the strip really turns. `path` replaces `position` and `extent`; setting both warns and uses the path. Two corners is exactly equivalent to a straight `position` + `extent` strip, so there is no reason to use both forms. A `path` with a single corner is just a point.

Where a leg runs affects what varies along it. In the example the wall leg climbs, so it sweeps bass to treble by height; the ceiling leg stays at one height and instead follows the panning as it crosses the room from left to right.

### Gradients on addressable strips

On a light that can only show one color — a Shelly, a Govee bulb — the strip's sample points are averaged down to that single color, and the paragraph above is the whole story.

An **addressable** light ([`wled`](#type-wled), [`govee_dreamview`](#type-govee_dreamview), or a [`hue`](#type-hue) entry with several lights) instead keeps them apart: it is sampled once per segment, and each segment is colored from where that segment physically is. Nothing extra has to be configured — give the light a `position` and `extent` (or a [`path`](#strips-that-bend)) plus a `spatiality`, and the gradient falls out of the same geometry:

```json
{
  "type": "wled", "host": "192.168.178.90",
  "leds": 120, "segments": 16,
  "form": "strip",
  "position": [-2.0, 1.2, 0.5],
  "extent":   [ 0.0, 1.2, 0.0],
  "spatiality": 1.0
}
```

- A **vertical** strip has no left/right spread, so its gradient comes from the height mapping: bass at the bottom, treble at the top.
- A **horizontal** strip across the room picks up the panning instead: its left end follows the left channel, its right end the right.
- Without a `position` or a `path` there is nothing to vary along the strip, so every segment gets the same color and the strip behaves like any other light.

`segments` is how many colors the strip shows, not how many LEDs it has. On WLED the pixels are divided evenly between them, so 120 LEDs with `segments: 16` gives runs of 7 or 8 pixels. More segments mean a smoother gradient and a slightly larger packet; the whole strip still goes out in one UDP write.

### Height and frequency

Left/right and front/back come from the audio itself. Height usually cannot: a stereo or 5.1 device carries no height information whatsoever.

So height is mapped onto **frequency** instead. A light near the floor leans toward the bass bands, one near the ceiling toward the treble bands, scaled across `spatial.room`'s `min.y` to `max.y`. `height_sharpness` controls how narrowly a height picks its bands.

If your layout does have height speakers, this stand-in is unnecessary and switches itself off. That is what `elevation_tilt: null` means: `1.0` when the layout has no height channels, `0.0` when it has. Set it to a number to decide yourself.

Note that this only shifts a light's *hue*. Brightness still comes from the overall level, so a floor light does not go dark just because the music has no bass.

### Tuning

- **The lights barely differ:** raise `spatiality` toward `1.0`, raise `focus`, and lower `omni_floor`. Also check that the music is actually panned — a mono master will look identical everywhere no matter what you configure.
- **A light is too isolated or goes dark:** lower `focus` or raise `omni_floor`.
- **Only left/right responds:** your output device is stereo. Check the layout shown in the **Audio** tab.
- **The colors drifted after adding positions:** that is the height mapping. Set `elevation_tilt: 0.0` on a light to keep its position purely horizontal.
- **An addressable strip shows one flat color:** it has no `position` or `path`, or its `spatiality` is still `0.0`. Without those there is nothing to vary along its length. Check the startup line — a strip that will show a gradient says so: `12 segment gradient`.
- **A WLED strip drops back to its own effect while music plays:** `realtime_timeout_s` is shorter than the gap between packets. Raise it, or lower `output.change_interval_ms` so frames arrive more often.
- **Part of a bent strip does not change color:** a leg that stays at one height has no bass-to-treble sweep along it, only whatever panning crosses it. Check the shape printed at startup against how the strip really runs.
- **A Govee strip ignores `segments`:** expected on most models — see the note under [`govee_dreamview`](#type-govee_dreamview). Set `segments: 1`.

## Upgrading from an older config

Older versions kept `change_interval_ms`, `audio_device`, `transition_min_ms`, `transition_max_ms`, `beat_threshold` and `strobe_ms` at the top level, and lights in `shellys[]` / `govees[]`. Both layouts are migrated automatically on the first start: your values move into the new sections, the old keys disappear, and the new sections appear with their defaults. Nothing needs to be edited by hand.

**Two behavioral changes:**

- Colors used to come from a fixed hue calculation over exactly three bands. They now come from the band and stop mapping described above. With the default configuration the assignment is the same — bass red, mids green, treble blue — but transitions are smoother.
- Band energies are now measured against one shared reference level (`dynamics.normalize: "shared"`). The old per-band normalization gave every band its own rolling peak, which meant a sustained sound drove all bands to their maximum at once regardless of how loud they really were relative to each other; the resulting mix carried no hue. Set `dynamics.normalize` to `per_band` to get the old measurement back.
- Brightness comes from the loudest band (`dynamics.level_source: "peak"`) instead of the mean across bands. With the shared reference level above, the mean would read a bass-only passage — one full band, the rest near zero — as quiet and dim as the lights. Set `level_source` to `average` for the mean.

The `audio.apps` block is new and appears with `enabled: false`, so an existing config keeps capturing the output device until you turn it on.

The `spatial` block is new and also appears with `enabled: false`. Your existing `devices[]` entries are left untouched — they gain no position keys — and every light keeps following the global mix until you add a `position` or a `path` and raise its `spatiality`. See [3D positioning](#3d-positioning).

## Troubleshooting

- **No audio detected:** check that `audio.device` points at the output you actually play through. The **Audio** tab lists every device.
- **Linux: the lights follow your microphone:** no monitor source was found, so a plain input was taken instead — the startup log says so. Check that PulseAudio or PipeWire is running with `pactl info`, then look in the **Audio** tab for a monitor device.
- **Linux: no monitor source is listed:** cpal fell back to ALSA, which cannot see one. Install or start PipeWire or PulseAudio, or configure an `snd-aloop` device by hand.
- **Linux: the build fails on `alsa-sys`:** the ALSA headers are missing. `sudo apt install libasound2-dev` or `sudo dnf install alsa-lib-devel`.
- **macOS: nothing is captured:** macOS has no loopback of its own; a virtual audio driver has to be installed and selected. See [Capturing on macOS](#capturing-on-macos).
- **macOS: the audio goes silent when you select the virtual device:** that device is not connected to your speakers. Use a Multi-Output Device that contains both, as described in [Capturing on macOS](#capturing-on-macos).
- **A selected app does not drive the lights:** open the **Applications** tab while it plays and use the name listed there. An app is only found once its process runs, so give it up to `audio.apps.rescan_ms`. "System sounds" cannot be captured at all.
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
