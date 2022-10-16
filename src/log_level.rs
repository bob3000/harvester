use std::{borrow::Cow, fmt::Display};

use clap::ValueEnum;

/// LogLevel defines the severity of the logging event
#[derive(Debug, Copy, Clone, PartialEq, Eq, PartialOrd, Ord, ValueEnum)]
pub enum LogLevel {
    Debug,
    Info,
    Warn,
    Error,
}

impl Display for LogLevel {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{:?}", self)
    }
}

impl From<&LogLevel> for Cow<'static, str> {
    fn from(value: &LogLevel) -> Self {
        Cow::Owned(value.to_string())
    }
}
