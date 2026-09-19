use crate::config::Downmix;
#[cfg(target_os = "windows")]
use std::slice::from_raw_parts;

#[cfg(target_os = "windows")]
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