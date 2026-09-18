#![allow(dead_code)]

use crate::config::AudioDeviceSelector;

pub const LINUX_LOOPBACK_HINTS: &[&str] = &["monitor", "loopback"];
pub const MACOS_LOOPBACK_HINTS: &[&str] = &["blackhole", "soundflower", "loopback audio", "ishowu", "vb-cable", "existential audio", "aggregate", "multi-output"];

pub fn is_loopback(name: &str, hints: &[&str]) -> bool { hints.iter().any(|h| name.to_lowercase().contains(h)) }

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SelectError { NoDevices, NoMatch { wanted: String }, }
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Selection { pub index: usize, pub fell_back_to_input: bool, }

pub fn choose(names: &[String], sel: &AudioDeviceSelector, hints: &[&str], default_index: Option<usize>) -> Result<Selection, SelectError> {
    if names.is_empty() { return Err(SelectError::NoDevices); }
    let found = |index: usize| Ok(Selection { index, fell_back_to_input: false });

    match sel {
        AudioDeviceSelector::Id { id } => names.iter().position(|n| n == id).or_else(|| names.iter().position(|n| n.eq_ignore_ascii_case(id)))
            .map(|i| Selection { index: i, fell_back_to_input: false }).ok_or_else(|| SelectError::NoMatch { wanted: id.clone() }),
        AudioDeviceSelector::Name { name } => {
            let wanted = name.to_lowercase();
            names.iter().position(|n| n.to_lowercase().contains(&wanted)).map(|i| Selection { index: i, fell_back_to_input: false }).ok_or_else(|| SelectError::NoMatch { wanted: name.clone() })
        }
        AudioDeviceSelector::Default => {
            if let Some(i) = names.iter().position(|n| is_loopback(n, hints)) { return found(i); }
            let index = default_index.filter(|i| *i < names.len()).unwrap_or(0);
            Ok(Selection { index, fell_back_to_input: true })
        }
    }
}

pub fn describe_devices(names: &[String], hints: &[&str], default_index: Option<usize>) -> String {
    if names.is_empty() {
        return "  (no capture devices found)".to_string();
    }
    names.iter().enumerate().map(|(i, name)| {
        let mut tags = Vec::new();
        if is_loopback(name, hints) {
            tags.push("system output");
        }
        if Some(i) == default_index {
            tags.push("default input");
        }
        match tags.is_empty() { 
            true => format!("  - {name}"),
            false => format!("  - {name}  [{}]", tags.join(", ")),
        }
    }).collect::<Vec<_>>().join("\n")
}