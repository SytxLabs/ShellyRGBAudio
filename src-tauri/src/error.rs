use serde::{Serialize, Serializer, ser::SerializeStruct};

#[derive(Debug, thiserror::Error)]
pub enum AppError {
    #[error("{0}")]
    Config(String),
    #[error("{0}")]
    Engine(String),
    #[error("{0}")]
    Device(String),
    #[error("{0}")]
    Io(String),
}

impl AppError {
    fn kind(&self) -> &'static str { match self {
        Self::Config(_) => "config",
        Self::Engine(_) => "engine",
        Self::Device(_) => "device",
        Self::Io(_) => "io",
    }}
}

impl Serialize for AppError {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        let mut s = serializer.serialize_struct("AppError", 2)?;
        s.serialize_field("kind", self.kind())?;
        s.serialize_field("message", &self.to_string())?;
        s.end()
    }
}

pub type AppResult<T> = Result<T, AppError>;

pub fn config_err(e: anyhow::Error) -> AppError {
    AppError::Config(format!("{e:#}"))
}
pub fn engine_err(e: anyhow::Error) -> AppError {
    AppError::Engine(format!("{e:#}"))
}
pub fn device_err(e: anyhow::Error) -> AppError {
    AppError::Device(format!("{e:#}"))
}
pub fn io_err(e: anyhow::Error) -> AppError {
    AppError::Io(format!("{e:#}"))
}
