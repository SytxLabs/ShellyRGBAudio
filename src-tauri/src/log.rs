use std::{fmt::Arguments, sync::{Arc, OnceLock, RwLock}};
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
#[cfg_attr(feature = "ts", derive(ts_rs::TS), ts(export))]
pub enum LogLevel { Info, Warn, Error, }

pub type LogSink = Arc<dyn Fn(LogLevel, &str) + Send + Sync>;

static SINK: OnceLock<RwLock<Option<LogSink>>> = OnceLock::new();

fn slot() -> &'static RwLock<Option<LogSink>> {
    SINK.get_or_init(|| RwLock::new(None))
}
pub fn set_sink(sink: LogSink) {
    *slot().write().unwrap_or_else(|e| e.into_inner()) = Some(sink);
}
pub fn clear_sink() {
    *slot().write().unwrap_or_else(|e| e.into_inner()) = None;
}
pub fn emit(level: LogLevel, args: Arguments<'_>) {
    let sink = slot().read().unwrap_or_else(|e| e.into_inner()).clone();
    match sink {
        Some(sink) => sink(level, &args.to_string()),
        None => eprintln!("{args}"),
    }
}

#[macro_export]
macro_rules! log_info {
    ($($arg:tt)*) => { $crate::log::emit($crate::log::LogLevel::Info, format_args!($($arg)*)) };
}

#[macro_export]
macro_rules! log_warn {
    ($($arg:tt)*) => { $crate::log::emit($crate::log::LogLevel::Warn, format_args!($($arg)*)) };
}

#[macro_export]
macro_rules! log_error {
    ($($arg:tt)*) => { $crate::log::emit($crate::log::LogLevel::Error, format_args!($($arg)*)) };
}
