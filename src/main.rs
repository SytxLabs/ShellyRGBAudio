mod config;
mod shelly;

use anyhow::{anyhow, Context, Result};
use rustfft::{num_complex::Complex32, FftPlanner};
use std::{
    sync::{mpsc, Arc},
    thread,
    time::{Duration, Instant},
};
use std::sync::atomic::{AtomicBool, Ordering};
use wasapi::{initialize_mta, Device, DeviceEnumerator, Direction, SampleType, StreamMode, WaveFormat};
use crate::config::AudioDeviceSelector;

#[derive(Clone, Copy, Debug)]
struct RgbwGain {
    r: u8,
    g: u8,
    b: u8,
    w: u8,
    gain: u8, // 0..=100  (brightness)
    transition_ms: u16,
}

fn main() -> Result<()> {
    // ---- Config ----
    let cfg = config::load_or_create("config.json")?;

    // Throttle / smooth interval from config
    let min_send_interval = Duration::from_millis(cfg.change_interval_ms);

    // Use the same value for Shelly transition by default (cap to u16)
    let transition_ms: u16 = cfg.change_interval_ms.min(u16::MAX as u64) as u16;

    // Brightness clamp range (keep some minimum so it doesn't go fully dark)
    let gain_min: f32 = 10.0;
    let gain_max: f32 = 100.0;

    // ---- Shelly controller (Gen1 RGBW2 + Gen2 Plus RGBW PM) ----
    let shelly = Arc::new(shelly::ShellyController::new(&cfg.shelly)?);
    let initial_state = match shelly.get_state() {
        Ok(s) => {
            eprintln!("Shelly initial state: {:?}", s);
            Some(s)
        }
        Err(e) => {
            eprintln!("Warn: could not read initial Shelly state: {e:#}");
            None
        }
    };
    let running = Arc::new(AtomicBool::new(true));
    {
        let running = Arc::clone(&running);
        ctrlc::set_handler(move || {
            running.store(false, Ordering::SeqCst);
        })?;
    }
    let old_hook = std::panic::take_hook();
    {
        let shelly = Arc::clone(&shelly);
        let initial_state = initial_state; // move
        std::panic::set_hook(Box::new(move |info| {
            if let Some(s) = initial_state {
                let _ = shelly.restore_state(s);
            }
            old_hook(info);
        }));
    }
    let initial_state = initial_state;

    // ---- FFT settings ----
    let fft_size: usize = 1024;
    let max_sample_rate_assumption: u32 = 48_000;

    // ---- Thread: Shelly sender ----
    let (tx, rx) = mpsc::channel::<RgbwGain>();
    let _sender_thread = {
        let shelly = Arc::clone(&shelly);
        thread::spawn(move || {
            let mut last_sent = Instant::now() - min_send_interval;
            let mut last = RgbwGain {
                r: 0,
                g: 0,
                b: 0,
                w: 0,
                gain: 0,
                transition_ms,
            };

            while let Ok(mut v) = rx.recv() {
                // Drain queue to keep only latest value
                while let Ok(newer) = rx.try_recv() {
                    v = newer;
                }

                // Throttle
                if last_sent.elapsed() < min_send_interval {
                    continue;
                }

                // De-dupe to reduce traffic
                let changed = (v.r as i16 - last.r as i16).abs() > 3
                    || (v.g as i16 - last.g as i16).abs() > 3
                    || (v.b as i16 - last.b as i16).abs() > 3
                    || (v.gain as i16 - last.gain as i16).abs() > 2;

                if !changed {
                    continue;
                }

                v.transition_ms = transition_ms;

                if let Err(e) = shelly.set_rgbw(v.r, v.g, v.b, v.w, v.gain, v.transition_ms as u32) {
                    eprintln!("Shelly send error: {e:#}");
                } else {
                    last = v;
                    last_sent = Instant::now();
                }
            }
        })
    };

    // ---- Audio capture (loopback) ----
    initialize_mta()
        .ok()
        .context("initialize_mta failed (COM init; avoid calling from STA UI thread)")?;

    let enumerator = DeviceEnumerator::new()?;
    let audio_err: Option<anyhow::Error> = match &cfg.audio_device {
        AudioDeviceSelector::Id { id } => {
            if id.is_empty() {
                eprintln!("Warn: audio device ID is empty, using default device instead.");
                list_render_devices(&enumerator);
                Some(anyhow!("Audio device ID must not be empty"))
            } else {
                None
            }
        }
        AudioDeviceSelector::Name { name } => {
            if name.is_empty() {
                eprintln!("Warn: audio device name is empty, using default device instead.");
                list_render_devices(&enumerator);
                Some(anyhow!("Audio device name must not be empty"))
            } else {
                None
            }
        }
        _ => None
    };
    if let Some(e) = audio_err {
        eprintln!("{e:#}");
        return Ok(());
    }
    let device = match select_render_device(&enumerator, &cfg.audio_device) {
        Ok(d) => d,
        Err(e) => {
            eprintln!("Audio device selection failed: {e:#}");
            list_render_devices(&enumerator);
            return Err(e);
        }
    };
    let mut audio_client = device.get_iaudioclient()?;

    // You can also do: let desired_format = audio_client.get_mixformat()?;
    // Keeping your original "force float stereo" approach:
    let desired_format = WaveFormat::new(
        32,
        32,
        &SampleType::Float,
        max_sample_rate_assumption as usize,
        2,
        None,
    );

    let buffer_duration_hns = 200_000; // 20ms in 100ns units
    let autoconvert = true;
    let mode = StreamMode::EventsShared {
        autoconvert,
        buffer_duration_hns,
    };

    audio_client.initialize_client(&desired_format, &Direction::Capture, &mode)?;
    let capture = audio_client.get_audiocaptureclient()?;
    let event = audio_client.set_get_eventhandle()?;
    audio_client.start_stream()?;

    // ---- FFT planner ----
    let mut planner = FftPlanner::<f32>::new();
    let fft = planner.plan_fft_forward(fft_size);

    let channels = desired_format.get_nchannels() as usize;
    let bytes_per_frame = channels * std::mem::size_of::<f32>();

    let mut mono_ring: Vec<f32> = Vec::with_capacity(fft_size);
    let mut fft_buf: Vec<Complex32> = vec![Complex32::new(0.0, 0.0); fft_size];

    // Running normalization
    let mut bass_peak = 1e-6f32;
    let mut mid_peak = 1e-6f32;
    let mut treble_peak = 1e-6f32;

    loop {
        if !running.load(Ordering::SeqCst) {
            break;
        }
        // Wait for event-driven capture timing
        event.wait_for_event(2000)?;

        while let Some(frames) = capture.get_next_packet_size()? {
            if frames == 0 {
                break;
            }

            let mut raw = vec![0u8; frames as usize * bytes_per_frame];
            let (read_frames, _info) = capture.read_from_device(&mut raw)?;
            if read_frames == 0 {
                break;
            }

            // Interleaved f32 audio
            let floats: &[f32] = bytemuck::cast_slice(&raw);

            for frame in floats.chunks_exact(channels) {
                let mono = if frame.len() == 1 {
                    frame[0]
                } else {
                    (frame[0] + frame[1]) * 0.5
                };

                mono_ring.push(mono);

                if mono_ring.len() >= fft_size {
                    // Hann window
                    for i in 0..fft_size {
                        let w = 0.5
                            - 0.5
                            * ((2.0 * std::f32::consts::PI * i as f32) / (fft_size as f32))
                            .cos();
                        fft_buf[i] = Complex32::new(mono_ring[i] * w, 0.0);
                    }
                    mono_ring.clear();

                    fft.process(&mut fft_buf);

                    let sr = desired_format.get_samplespersec().max(1) as f32;
                    let mut bass = 0.0f32;
                    let mut mid = 0.0f32;
                    let mut treble = 0.0f32;

                    let half = fft_size / 2;
                    for bin in 1..half {
                        let freq = (bin as f32) * sr / (fft_size as f32);
                        let mag2 = fft_buf[bin].norm_sqr();

                        if (20.0..200.0).contains(&freq) {
                            bass += mag2;
                        } else if (200.0..2000.0).contains(&freq) {
                            mid += mag2;
                        } else if (2000.0..8000.0).contains(&freq) {
                            treble += mag2;
                        }
                    }

                    // Compression
                    bass = (bass + 1.0).ln();
                    mid = (mid + 1.0).ln();
                    treble = (treble + 1.0).ln();

                    // Peaks with slow decay
                    bass_peak = bass_peak.max(bass) * 0.995;
                    mid_peak = mid_peak.max(mid) * 0.995;
                    treble_peak = treble_peak.max(treble) * 0.995;

                    let rb = (bass / bass_peak).clamp(0.0, 1.0);
                    let gm = (mid / mid_peak).clamp(0.0, 1.0);
                    let bt = (treble / treble_peak).clamp(0.0, 1.0);

                    // Brightness from overall energy
                    let overall = ((rb + gm + bt) / 3.0).clamp(0.0, 1.0);

                    let hue = bands_to_hue(rb, gm, bt);

                    let value = (0.15 + 0.85 * overall).clamp(0.0, 1.0);

                    let (r, g, b) = hue_to_two_channel_rgb(hue, value);

                    let gain_f = gain_min + overall * (gain_max - gain_min);
                    let gain = gain_f.round().clamp(gain_min, gain_max) as u8;

                    let out = RgbwGain {
                        r,
                        g,
                        b,
                        w: 0,
                        gain,
                        transition_ms,
                    };

                    let _ = tx.send(out);
                }
            }
        }
    }

    if let Some(s) = initial_state {
        eprintln!("Restoring Shelly state...");
        if let Err(e) = shelly.restore_state(s) {
            eprintln!("Restore failed: {e:#}");
        }
    }
    Ok(())
}

// Minimal cast helper (you can swap this for the bytemuck crate if you want)
mod bytemuck {
    pub fn cast_slice<T: Copy, U: Copy>(data: &[T]) -> &[U] {
        let byte_ptr = data.as_ptr() as *const U;
        let byte_len = std::mem::size_of_val(data);
        let new_len = byte_len / std::mem::size_of::<U>();
        unsafe { std::slice::from_raw_parts(byte_ptr, new_len) }
    }
}
fn list_render_devices(enumerator: &DeviceEnumerator) {
    if let Ok(coll) = enumerator.get_device_collection(&Direction::Render) {
        eprintln!("--- Render devices (Output) ---");
        for dev_res in &coll {
            if let Ok(dev) = dev_res {
                let name = dev.get_friendlyname().unwrap_or_else(|_| "<no name>".to_string());
                let id = dev.get_id().unwrap_or_else(|_| "<no id>".to_string());
                eprintln!("  - {name}\n    id: {id}");
            }
        }
    }
}

fn select_render_device(enumerator: &DeviceEnumerator, sel: &AudioDeviceSelector) -> Result<Device> {
    match sel {
        AudioDeviceSelector::Default => Ok(enumerator.get_default_device(&Direction::Render)?),

        AudioDeviceSelector::Id { id } => Ok(enumerator.get_device(id)?),

        AudioDeviceSelector::Name { name } => {
            let coll = enumerator.get_device_collection(&Direction::Render)?;

            // 1) erst versuchen: contains-match (praktischer als exact)
            for dev_res in &coll {
                let dev = dev_res?;
                let fname = dev.get_friendlyname().unwrap_or_default();
                if fname.to_lowercase().contains(&name.to_lowercase()) {
                    return Ok(dev);
                }
            }

            // 2) falls du lieber exact willst: coll.get_device_with_name(name)
            // (kann je nach Implementierung exact match erwarten)
            // return Ok(coll.get_device_with_name(name)?);

            anyhow::bail!("Audio device not found by name: {name}");
        }
    }
}

fn hue_to_two_channel_rgb(hue_deg: f32, value: f32) -> (u8, u8, u8) {
    let hue = hue_deg.rem_euclid(360.0);
    let v = value.clamp(0.0, 1.0);

    let (r, g, b) = if hue < 120.0 {
        let t = hue / 120.0;          // 0..1
        (1.0, t, 0.0)                 // R->RG
    } else if hue < 240.0 {
        let t = (hue - 120.0) / 120.0;
        (0.0, 1.0, t)                 // G->GB
    } else {
        let t = (hue - 240.0) / 120.0;
        (t, 0.0, 1.0)                 // B->BR
    };

    let rr = (r * v * 255.0).round() as u8;
    let gg = (g * v * 255.0).round() as u8;
    let bb = (b * v * 255.0).round() as u8;
    (rr, gg, bb)
}

// Hue aus (rb, gm, bt) "smooth" ableiten (kein harter switch)
fn bands_to_hue(rb: f32, gm: f32, bt: f32) -> f32 {
    // 2D-Projektion der 3 Anteile -> Winkel
    let x = rb - 0.5 * (gm + bt);
    let y = (3.0_f32.sqrt() / 2.0) * (gm - bt);
    let mut hue = y.atan2(x) * 180.0 / std::f32::consts::PI;
    if hue < 0.0 { hue += 360.0; }
    hue
}
