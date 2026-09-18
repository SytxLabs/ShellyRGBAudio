use anyhow::{anyhow, bail, Context, Result};
use cpal::traits::{DeviceTrait, HostTrait, StreamTrait};
use cpal::{FromSample, Sample, SampleFormat, SizedSample, Stream, StreamConfig};
use std::sync::mpsc::{self, Receiver, SyncSender, TryRecvError};
use std::time::Duration;

use crate::capture::select::{choose, describe_devices, is_loopback, SelectError};
use crate::capture::{AppInfo, AppTarget, CaptureStream};
use crate::config::{AppsSection, AudioDeviceSelector, AudioSection};
use crate::spatial::SpeakerLayout;

// The two things that differ between Linux and macOS, supplied by whichever of them included this file.
use super::{LOOPBACK_HINTS, NO_LOOPBACK_HELP};

pub const APP_CAPTURE_SUPPORTED: bool = false;

const READ_TIMEOUT: Duration = Duration::from_millis(100); // Kept short so a capture thread notices Ctrl+C quickly.
const QUEUE_CHUNKS: usize = 32; // Roughly a third of a second of audio in flight before the oldest is dropped.

struct CpalStream {
    _stream: Stream,
    rx: Receiver<Vec<f32>>,
    sample_rate: f32,
    channels: usize,
}

impl CaptureStream for CpalStream {
    fn sample_rate(&self) -> f32 {
        self.sample_rate
    }
    fn layout(&self) -> SpeakerLayout { SpeakerLayout::from_count(self.channels) }
    fn read_frames(&mut self, out: &mut Vec<f32>) -> Result<usize> {
        let before = out.len();
        match self.rx.recv_timeout(READ_TIMEOUT) {
            Ok(chunk) => out.extend_from_slice(&chunk),
            Err(mpsc::RecvTimeoutError::Timeout) => return Ok(0),
            Err(mpsc::RecvTimeoutError::Disconnected) => bail!("capture stream ended"),
        }
        loop {
            match self.rx.try_recv() {
                Ok(chunk) => out.extend_from_slice(&chunk),
                Err(TryRecvError::Empty) => break,
                Err(TryRecvError::Disconnected) => break,
            }
        }
        Ok((out.len() - before) / self.channels.max(1))
    }
}
fn input_devices() -> Result<(Vec<cpal::Device>, Vec<String>, Option<usize>)> {
    let host = cpal::default_host();
    let devices: Vec<cpal::Device> = host.input_devices().context("could not list the capture devices")?.collect();
    let names: Vec<String> = devices.iter().map(|d| d.to_string()).collect();
    Ok((devices, names, host.default_input_device().map(|d| d.to_string()).and_then(|name| names.iter().position(|n| *n == name))))
}

pub fn list_devices() {
    let (_, names, default) = match input_devices() {
        Ok(found) => found,
        Err(e) => {
            eprintln!("Could not list the audio devices: {e:#}");
            return;
        }
    };
    eprintln!("--- Capture devices (usable in audio.device) ---");
    eprintln!("{}", describe_devices(&names, LOOPBACK_HINTS, default));
    eprintln!();
    eprintln!("Pick one marked [system output]; that is what carries what you hear.");
    if !names.iter().any(|n| is_loopback(n, LOOPBACK_HINTS)) {
        eprintln!("{NO_LOOPBACK_HELP}");
    }
}

pub fn open_device(sel: &AudioDeviceSelector, _audio: &AudioSection) -> Result<Box<dyn CaptureStream>> {
    let (devices, names, default) = input_devices()?;

    let picked = match choose(&names, sel, LOOPBACK_HINTS, default) {
        Ok(p) => p,
        Err(SelectError::NoDevices) => bail!("no capture devices found. {NO_LOOPBACK_HELP}"),
        Err(SelectError::NoMatch { wanted }) => {
            eprintln!("--- Capture devices (usable in audio.device) ---");
            eprintln!("{}", describe_devices(&names, LOOPBACK_HINTS, default));
            bail!("no capture device matched {wanted:?}");
        }
    };
    if picked.fell_back_to_input {
        eprintln!("Warn: no capture device looks like the system's output, falling back to {:?}, which is probably a microphone.", names[picked.index]);
        eprintln!("{NO_LOOPBACK_HELP}");
    }

    let device = &devices[picked.index];
    let supported = device.default_input_config().with_context(|| format!("device {:?} reports no usable capture format", names[picked.index]))?;
    let config: StreamConfig = supported.config();
    let (channels, sample_rate) = (config.channels as usize, config.sample_rate as f32);
    if channels == 0 {
        bail!("device {:?} reports zero channels", names[picked.index]);
    }

    let (tx, rx) = mpsc::sync_channel::<Vec<f32>>(QUEUE_CHUNKS);
    let stream = match supported.sample_format() {
        SampleFormat::F32 => build::<f32>(device, config, tx),
        SampleFormat::I16 => build::<i16>(device, config, tx),
        SampleFormat::U16 => build::<u16>(device, config, tx),
        SampleFormat::I32 => build::<i32>(device, config, tx),
        SampleFormat::I8 => build::<i8>(device, config, tx),
        SampleFormat::U8 => build::<u8>(device, config, tx),
        SampleFormat::F64 => build::<f64>(device, config, tx),
        other => Err(anyhow!("sample format {other:?} is not supported")),
    }.with_context(|| format!("could not open capture device {:?}", names[picked.index]))?;
    stream.play().context("could not start the capture stream")?;
    Ok(Box::new(CpalStream { _stream: stream, rx, sample_rate, channels }))
}

/// Opens the stream for one concrete sample type, converting each buffer to `f32` as it arrives.
fn build<T>(device: &cpal::Device, config: StreamConfig, tx: SyncSender<Vec<f32>>) -> Result<Stream> where T: SizedSample, f32: FromSample<T>, {
    let stream = device.build_input_stream::<T, _, _>(
        config,
        move |data: &[T], _| { let _ = tx.try_send(data.iter().map(|s| f32::from_sample(*s)).collect()); },
        |e| eprintln!("Capture: stream error (ignored): {e}"),
        None,
    ).context("build_input_stream failed")?;
    Ok(stream)
}

const NO_APP_CAPTURE: &str = "capturing a single application is only implemented on Windows; set audio.apps.enabled to false to capture the output device instead";
pub fn list_apps() -> Result<Vec<AppInfo>> {
    bail!(NO_APP_CAPTURE)
}
pub fn resolve_targets(_apps: &AppsSection) -> Result<Vec<AppTarget>> {
    bail!(NO_APP_CAPTURE)
}
pub fn open_app(_target: &AppTarget, _audio: &AudioSection) -> Result<Box<dyn CaptureStream>> { bail!(NO_APP_CAPTURE) }
pub fn process_alive(_pid: u32) -> bool {
    false
}
