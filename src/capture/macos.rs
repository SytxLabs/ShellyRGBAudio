use crate::capture::select::MACOS_LOOPBACK_HINTS;

const LOOPBACK_HINTS: &[&str] = MACOS_LOOPBACK_HINTS;

const NO_LOOPBACK_HELP: &str = "\
macOS cannot record its own output without help. Install a virtual audio driver - BlackHole (https://existential.audio/blackhole) is free - then:
  1. Audio MIDI Setup > + > Create Multi-Output Device, tick both your speakers and BlackHole.
  2. Select that Multi-Output Device as the system output, so you still hear the audio.
  3. Set audio.device to {\"type\": \"name\", \"name\": \"BlackHole\"}, or leave it on default and it will be found.
The program also needs microphone permission the first time it records, which macOS asks for on the first run.";

#[path = "cpal_backend.rs"]
mod shared;

pub use shared::*;
