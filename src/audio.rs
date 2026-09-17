use anyhow::Result;
use std::slice::from_raw_parts;
use wasapi::{Device, DeviceEnumerator, Direction};

use crate::config::{AudioDeviceSelector, Downmix};

pub fn list_render_devices(enumerator: &DeviceEnumerator) {
    if let Ok(coll) = enumerator.get_device_collection(&Direction::Render) {
        eprintln!("--- Render devices (Output) ---");
        for dev_res in &coll {
            let Ok(dev) = dev_res else { continue };
            let name = dev.get_friendlyname().unwrap_or_else(|_| "<no name>".to_string());
            let id = dev.get_id().unwrap_or_else(|_| "<no id>".to_string());
            eprintln!("  - {name}\n    id: {id}");
        }
    }
}

pub fn select_render_device(enumerator: &DeviceEnumerator, sel: &AudioDeviceSelector) -> Result<Device> {
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

pub fn cast_slice<T: Copy, U: Copy>(data: &[T]) -> &[U] {
    let byte_ptr = data.as_ptr() as *const U;
    let byte_len = size_of_val(data);
    let new_len = byte_len / size_of::<U>();
    unsafe { from_raw_parts(byte_ptr, new_len) }
}

pub fn downmix(frame: &[f32], mode: Downmix) -> f32 {
    if frame.is_empty() {
        return 0.0;
    }
    match mode {
        Downmix::Average => {
            if frame.len() == 1 {
                frame[0]
            } else {
                (frame[0] + frame[1]) * 0.5
            }
        }
        Downmix::Left => frame[0],
        Downmix::Right => frame[frame.len().min(2) - 1],
        Downmix::AllChannels => frame.iter().sum::<f32>() / frame.len() as f32,
    }
}