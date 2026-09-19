use crate::capture::select::LINUX_LOOPBACK_HINTS;

const LOOPBACK_HINTS: &[&str] = LINUX_LOOPBACK_HINTS;

const NO_LOOPBACK_HELP: &str = "\
No monitor source was found. These come from PulseAudio or PipeWire, so check that one of them is running (`pactl info`).
On a plain ALSA system there is no way to record the output, and a loopback device has to be set up by hand (`snd-aloop`).";

#[path = "cpal_backend.rs"]
mod shared;

pub use shared::*;
