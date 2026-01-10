# ShellyToMusic

A high-performance Rust application that synchronizes your Shelly RGBW lights with your computer's audio output in real-time. It uses FFT (Fast Fourier Transform) to analyze audio frequencies and map them to colors and brightness on your Shelly devices.

## Features

- **Multi-Device Support:** Synchronize multiple Shelly devices simultaneously.
- **Low Latency:** Written in Rust with optimized audio capture and processing.
- **Cross-Generation Support:** Works with both Shelly Gen1 (RGBW2) and Gen2 (Plus RGBW PM) devices.
- **Auto-Detection:** Automatically detects the Shelly API version.
- **Advanced Audio Analysis:** Split audio into Bass, Mid, and Treble for dynamic color shifting.
- **Smart Throttling:** De-duplicates and throttles network requests to keep your Shelly responsive.
- **State Restoration:** Gracefully restores your lights to their previous state when the program exits.

## Installation

1. Ensure you have [Rust](https://www.rust-lang.org/tools/install) installed.
2. Clone this repository.
3. Build the project:
   ```bash
   cargo build --release
   ```

## Configuration

On the first run, the application creates a `config.json` file in the root directory. You can modify this file to suit your setup.

### Example `config.json`

```json
{
  "change_interval_ms": 120,
  "audio_device": {
    "type": "default"
  },
  "shellys": [
    {
      "host": "192.168.1.50",
      "device": "auto",
      "min_brightness": 1,
      "max_brightness": 80,
      "brightness_gamma": 0.6,
      "rgbw_id": 0,
      "auth": null
    }
  ]
}
```
### Config Settings
| Setting                      | Description                                                                                                                                                                                         |
|------------------------------|-----------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------|
| `change_interval_ms`         | Interval in milliseconds between color updates. Lower values increase responsiveness but may strain the Shelly device.                                                                              |
| `audio_device`               | Selection method for the audio output device. Can be `{"type": "default"}`, `{"type": "id", "id": "..."}`, or `{"type": "name", "name": "..."}`. If "name" or "id" is empty all devices are listed. |
| `shellys[].host`             | IP address or hostname of your Shelly device (e.g., `192.168.1.50`).                                                                                                                                |
| `shellys[].device`           | API version: `auto` (detects automatically), `rgbw2` (Gen1), or `plus_rgbw_pm` (Gen2).                                                                                                              |
| `shellys[].min_brightness`   | Lower limit for brightness (0-100).                                                                                                                                                                 |
| `shellys[].max_brightness`   | Upper limit for brightness (1-100).                                                                                                                                                                 |
| `shellys[].brightness_gamma` | Gamma correction for brightness. Values < 1 make low volumes brighter; > 1 make peaks more prominent.                                                                                               |
| `shellys[].rgbw_id`          | The component ID (usually 0). Relevant for Gen2 devices.                                                                                                                                            |
| `shellys[].auth`             | Optional: `{"username": "...", "password": "..."}` for password-protected devices.                                                                                                                  |

## Troubleshooting

- **No audio detected:** Ensure the application is using the correct audio output device. On Windows, this is typically your speakers or headphones.
- **Shelly not responding:** Check the `host` IP and ensure your computer can reach the Shelly device via the network.
- **Latency:** Try reducing `change_interval_ms` or checking your network stability.
- **Authentication issues:** If your Shelly device is password-protected, ensure the `auth` field in `config.json` is correctly set.
- **Shelly crashes:** Try increasing `change_interval_ms` or reducing `max_brightness`.
- **Other issues:** Open an issue on this repository.

## Support
Only Windows is supported at the moment.
