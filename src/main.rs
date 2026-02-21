mod config;
mod shelly;
mod govee;

use crate::config::AudioDeviceSelector;
use anyhow::{anyhow, Context, Result};
use rustfft::{num_complex::Complex32, FftPlanner};
use std::{ sync::{mpsc, Arc, atomic::{AtomicBool, Ordering}}, thread, time::{Duration, Instant} };
use wasapi::{initialize_mta, Device, DeviceEnumerator, Direction, StreamMode};

#[derive(Clone, Copy, Debug)]
struct RgbwGain { r: u8, g: u8, b: u8, w: u8, overall: f32, transition_ms: u16, force: bool }

enum RuntimeDevice {
    Shelly {
        cfg: config::ShellyConfig,
        ctrl: Arc<shelly::ShellyController>,
        initial: Option<shelly::ShellyRgbwState>,
    },
    Govee {
        cfg: config::GoveeLanConfig,
        initial: Option<govee::GoveeState>,
    },
}

fn main() -> Result<()> {
    let cfg = config::load_or_create("config.json")?;

    let mut runtime: Vec<RuntimeDevice> = Vec::new();

    let has_govee = cfg.devices.iter().any(|d| matches!(d, config::DeviceConfig::GoveeLan(_)));
    let govee_client = if has_govee { Some(Arc::new(govee::GoveeLan::new()?)) } else { None };

    for dev in &cfg.devices {
        match dev {
            config::DeviceConfig::Shelly(sc) => {
                let ctrl = Arc::new(shelly::ShellyController::new(sc)?);
                let initial = ctrl.get_state().ok();
                runtime.push(RuntimeDevice::Shelly { cfg: sc.clone(), ctrl, initial });
            }
            config::DeviceConfig::GoveeLan(gc) => {
                runtime.push(RuntimeDevice::Govee { cfg: gc.clone(), initial: govee_client.as_ref().and_then(|c| c.get_state(&gc.ip).ok()) });
            }
        }
    }

    let runtime = Arc::new(runtime);

    let min_send_interval = Duration::from_millis(cfg.change_interval_ms);
    let transition_min = cfg.transition_min_ms as f32;
    let transition_max = cfg.transition_max_ms as f32;

    let running = Arc::new(AtomicBool::new(true));
    {
        let running = Arc::clone(&running);
        ctrlc::set_handler(move || { running.store(false, Ordering::SeqCst); })?;
    }

    let old_hook = std::panic::take_hook();
    {
        let runtime = Arc::clone(&runtime);
        let govee_client_for_panic = govee_client.clone();

        std::panic::set_hook(Box::new(move |info| {
            restore_last_state(&runtime, &govee_client_for_panic);
            old_hook(info);
        }));
    }


    let mut dimmed_due_to_silence = false;
    let mut last_color: (u8, u8, u8, u8) = (255, 0, 0, 0);

    let fft_size: usize = 1024;

    let (tx, rx) = mpsc::channel::<RgbwGain>();
    let _sender_thread = {
        let runtime = Arc::clone(&runtime);
        let govee_client_for_sender = govee_client.clone();

        thread::spawn(move || {
            let mut last_sent = Instant::now() - min_send_interval;
            let mut last = RgbwGain { r: 0, g: 0, b: 0, w: 0, overall: 0.0, transition_ms: 0, force: false };

            while let Ok(mut v) = rx.recv() {
                while let Ok(newer) = rx.try_recv() { v = newer; }

                if last_sent.elapsed() < min_send_interval { continue; }
                let changed = (v.r as i16 - last.r as i16).abs() > 3 || (v.g as i16 - last.g as i16).abs() > 3 || (v.b as i16 - last.b as i16).abs() > 3 || (v.overall - last.overall).abs() > 0.02;
                if !changed && !v.force { continue; }

                for dev in runtime.iter() {
                    match dev {
                        RuntimeDevice::Shelly { cfg, ctrl, .. } => {
                            let minb = cfg.min_brightness.clamp(0, 100) as f32;
                            let shaped = v.overall.clamp(0.0, 1.0).powf(cfg.brightness_gamma.clamp(0.1, 5.0));
                            let mut bri = (minb + shaped * ((cfg.max_brightness.clamp(1, 100) as f32) - minb)).round() as u8;
                            if bri == 0 { bri = 1; }
                            let _ = ctrl.set_rgbw(v.r, v.g, v.b, v.w, bri, v.transition_ms as u32);
                        }

                        RuntimeDevice::Govee { cfg, .. } => {
                            let Some(g) = govee_client_for_sender.as_ref() else { continue; };

                            let minb = cfg.min_brightness.clamp(0, 100) as f32;
                            let shaped = v.overall.clamp(0.0, 1.0).powf(cfg.brightness_gamma.clamp(0.1, 5.0));
                            let mut bri = (minb + shaped * ((cfg.max_brightness.clamp(1, 100) as f32) - minb)).round() as u8;
                            if bri == 0 { bri = 1; }

                            let _ = g.turn(&cfg.ip, true);
                            let _ = g.set_brightness(&cfg.ip, bri);
                            let _ = g.set_rgb(&cfg.ip, v.r, v.g, v.b);
                        }
                    }
                }

                last = v;
                last_sent = Instant::now();
            }
        })
    };


    initialize_mta().ok().context("initialize_mta failed (COM init; avoid calling from STA UI thread)")?;

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
    let desired_format = audio_client.get_mixformat()?;

    let mode = StreamMode::EventsShared { autoconvert: true, buffer_duration_hns: 200_000 };
    audio_client.initialize_client(&desired_format, &Direction::Capture, &mode)?;
    let capture = audio_client.get_audiocaptureclient()?;
    let event = audio_client.set_get_eventhandle()?;
    audio_client.start_stream()?;

    // ---- FFT planner ----
    let mut planner = FftPlanner::<f32>::new();
    let fft = planner.plan_fft_forward(fft_size);

    let channels = desired_format.get_nchannels() as usize;
    let bytes_per_frame = channels * size_of::<f32>();

    let mut mono_ring: Vec<f32> = Vec::with_capacity(fft_size);
    let mut fft_buf: Vec<Complex32> = vec![Complex32::new(0.0, 0.0); fft_size];

    let mut bass_peak = 1e-6f32;
    let mut mid_peak = 1e-6f32;
    let mut treble_peak = 1e-6f32;

    let mut overall_ema: f32 = 0.0;
    let mut flux_ema: f32 = 0.0;

    let mut prev_bass: f32 = 0.0;
    let mut prev_mid: f32 = 0.0;
    let mut prev_treble: f32 = 0.0;

    let mut strobe_until: Option<Instant> = None;

    // Tuning
    let level_alpha: f32 = 0.12;  // Overall level smoothing (smaller = less, bigger = react)
    let flux_alpha: f32  = 0.25;  // Beat smoothing
    let beat_threshold: f32 = cfg.beat_threshold;
    let strobe_ms: u64 = cfg.strobe_ms;

    loop {
        if !running.load(Ordering::SeqCst) {
            break;
        }

        if let Err(_) = event.wait_for_event(2000) {
            if !dimmed_due_to_silence {
                let (r, g, b, w) = last_color;
                let _ = tx.send(RgbwGain { r, g, b, w, overall: 0.0, transition_ms: 800, force: true});
                dimmed_due_to_silence = true;
            }
            continue;
        }

        while let Some(frames) = capture.get_next_packet_size()? {
            if frames == 0 {
                break;
            }

            let mut raw = vec![0u8; frames as usize * bytes_per_frame];
            let (read_frames, _info) = capture.read_from_device(&mut raw)?;
            if read_frames == 0 {
                break;
            }

            let floats: &[f32] = cast_slice(&raw);
            for frame in floats.chunks_exact(channels) {
                let mono = if frame.len() == 1 { frame[0] } else { (frame[0] + frame[1]) * 0.5 };

                mono_ring.push(mono);
                if mono_ring.len() >= fft_size {
                    for i in 0..fft_size {
                        let w = 0.5 - 0.5 * ((2.0 * std::f32::consts::PI * i as f32) / (fft_size as f32)).cos();
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

                    bass = (bass + 1.0).ln();
                    mid = (mid + 1.0).ln();
                    treble = (treble + 1.0).ln();

                    bass_peak = bass_peak.max(bass) * 0.995;
                    mid_peak = mid_peak.max(mid) * 0.995;
                    treble_peak = treble_peak.max(treble) * 0.995;

                    let rb = (bass / bass_peak).clamp(0.0, 1.0);
                    let gm = (mid / mid_peak).clamp(0.0, 1.0);
                    let bt = (treble / treble_peak).clamp(0.0, 1.0);

                    let flux = ((rb - prev_bass).max(0.0) + (gm - prev_mid).max(0.0) + (bt - prev_treble).max(0.0)).clamp(0.0, 3.0) / 3.0;
                    prev_bass = rb;
                    prev_mid = gm;
                    prev_treble = bt;
                    flux_ema += flux_alpha * (flux - flux_ema);

                    // Brightness from overall energy
                    let overall = ((rb + gm + bt) / 3.0).clamp(0.0, 1.0);
                    overall_ema += level_alpha * (overall - overall_ema);
                    let now = Instant::now();

                    if flux_ema > beat_threshold { strobe_until = Some(now + Duration::from_millis(strobe_ms)); }
                    let strobing = strobe_until.map(|t| now < t).unwrap_or(false);
                    let beat_strength = ((flux_ema - beat_threshold) / (1.0 - beat_threshold)).clamp(0.0, 1.0);
                    let mix = (0.75 * beat_strength + 0.25 * overall_ema).clamp(0.0, 1.0);
                    let transition_f = transition_max - mix * (transition_max - transition_min);
                    let transition_ms = transition_f.round().clamp(0.0, u16::MAX as f32) as u16;

                    let overall = if strobing { 1.0 } else { overall_ema };
                    let (r, g, b) = hue_to_two_channel_rgb(bands_to_hue(rb, gm, bt), (0.15 + 0.85 * overall).clamp(0.0, 1.0));

                    if bass != 0.0 && mid != 0.0 && treble != 0.0 {
                        dimmed_due_to_silence = false;
                        last_color = (r, g, b, 0);
                    }
                    let _ = tx.send(RgbwGain { r, g, b, w: 0, overall, transition_ms, force: false});
                }
            }
        }
    }
    restore_last_state(&runtime, &govee_client);
    Ok(())
}

fn restore_last_state(runtime: &[RuntimeDevice], govee_client: &Option<Arc<govee::GoveeLan>>) {
    for dev in runtime.iter() {
        match dev {
            RuntimeDevice::Shelly { ctrl, initial, .. } => {
                if let Some(st) = initial.clone() {
                    let _ = ctrl.restore_state(st);
                }
            }
            RuntimeDevice::Govee { cfg, initial } => {
                if let (Some(g), Some(st)) = (govee_client.as_ref(), initial.clone()) {
                    let _ = g.restore_state(&cfg.ip, st);
                }
            }
        }
    }
}


fn cast_slice<T: Copy, U: Copy>(data: &[T]) -> &[U] {
    let byte_ptr = data.as_ptr() as *const U;
    let byte_len = size_of_val(data);
    let new_len = byte_len / size_of::<U>();
    unsafe { std::slice::from_raw_parts(byte_ptr, new_len) }
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
            for dev_res in &coll {
                let dev = dev_res?;
                let f_name = dev.get_friendlyname().unwrap_or_default();
                if f_name.to_lowercase().contains(&name.to_lowercase()) {
                    return Ok(dev);
                }
            }
            anyhow::bail!("Audio device not found by name: {name}");
        }
    }
}

fn hue_to_two_channel_rgb(hue_deg: f32, value: f32) -> (u8, u8, u8) {
    let hue = hue_deg.rem_euclid(360.0);
    let v = value.clamp(0.0, 1.0);

    let (r, g, b) = if hue < 120.0 {
        (1.0, hue / 120.0, 0.0)                 // R->RG
    } else if hue < 240.0 {
        (0.0, 1.0, (hue - 120.0) / 120.0)       // G->GB
    } else {
        ((hue - 240.0) / 120.0, 0.0, 1.0)       // B->BR
    };
    ((r * v * 255.0).round() as u8, (g * v * 255.0).round() as u8, (b * v * 255.0).round() as u8)
}

fn bands_to_hue(rb: f32, gm: f32, bt: f32) -> f32 {
    let mut hue = ((3.0_f32.sqrt() / 2.0) * (gm - bt)).atan2(rb - 0.5 * (gm + bt)) * 180.0 / std::f32::consts::PI;
    if hue < 0.0 { hue += 360.0; }
    hue
}
