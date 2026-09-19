use crate::paths::Language;

pub fn effective(language: Language) -> Language {
    match language {
        Language::System => Language::English,
        other => other,
    }
}

macro_rules! strings {
    ($($name:ident => $en:literal / $de:literal),* $(,)?) => {
        $(
            pub fn $name(language: Language) -> &'static str {
                match effective(language) {
                    Language::German => $de,
                    _ => $en,
                }
            }
        )*
    };
}

strings! {
    pause      => "Pause"    / "Pause",
    resume     => "Resume"   / "Fortsetzen",
    settings   => "Settings…" / "Einstellungen …",
    reload     => "Reload"   / "Neu laden",
    quit       => "Quit"     / "Beenden",
    starting   => "Starting…" / "Startet …",
    stopped    => "Paused"   / "Angehalten",
    running    => "Running"  / "Läuft",
    unknown_error => "unknown error" / "unbekannter Fehler",
}

pub fn error_prefix(language: Language, reason: &str) -> String {
    match effective(language) {
        Language::German => format!("Fehler: {reason}"),
        _ => format!("Error: {reason}"),
    }
}

pub fn device_count(language: Language, count: usize) -> String {
    match (effective(language), count) {
        (Language::German, 1) => "1 Gerät".to_string(),
        (Language::German, n) => format!("{n} Geräte"),
        (_, 1) => "1 device".to_string(),
        (_, n) => format!("{n} devices"),
    }
}
