/// Shared diagnostic types used by all stages.
use serde::Serialize;

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum Level {
    Info,
    Warning,
    Error,
}

impl std::fmt::Display for Level {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Level::Info => write!(f, "info"),
            Level::Warning => write!(f, "warning"),
            Level::Error => write!(f, "error"),
        }
    }
}

#[derive(Debug, Clone, Serialize)]
pub struct Diagnostic {
    pub level: Level,
    pub code: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub addr: Option<u16>,
    pub message: String,
}

impl Diagnostic {
    pub fn error(code: &str, addr: u16, message: impl Into<String>) -> Self {
        Self {
            level: Level::Error,
            code: code.into(),
            addr: Some(addr),
            message: message.into(),
        }
    }
    pub fn warning(code: &str, addr: u16, message: impl Into<String>) -> Self {
        Self {
            level: Level::Warning,
            code: code.into(),
            addr: Some(addr),
            message: message.into(),
        }
    }
    pub fn info(code: &str, addr: u16, message: impl Into<String>) -> Self {
        Self {
            level: Level::Info,
            code: code.into(),
            addr: Some(addr),
            message: message.into(),
        }
    }
}
