use std::time::Instant;

use chrono::{DateTime, Utc};

use crate::log_level::LogLevel;

pub const DEFAULT_TAG: &str = "GLOBAL";

#[derive(Debug, Clone)]
pub struct LogData {
    pub level: LogLevel,
    pub tag: Option<String>,
    pub message: String,
    pub timestamp: DateTime<Utc>,

    pub file: String,
    pub line: u32,
    pub column: u32,
    pub function_name: Option<String>,
}

impl LogData {
    pub fn new(
        level: LogLevel,
        tag: Option<String>,
        message: String,
        file: String,
        line: u32,
        column: u32,
        function_name: Option<String>,
    ) -> Self {
        Self {
            level,
            tag,
            message,
            timestamp: Utc::now(),
            file,
            line,
            column,
            function_name,
        }
    }

    pub fn format(&self) -> String {
        format!(
            "{level} {time} [{tag}] [{file}:{line}:{column} @ {function_name}] {message}",
            level = self.level,
            time = self.timestamp.format("%Y-%m-%d %H:%M:%S"),
            tag = self.tag.as_deref().unwrap_or(DEFAULT_TAG),
            message = self.message,
            line = self.line,
            column = self.column,
            file = self.file,
            function_name = self.function_name.as_deref().unwrap_or("default")
        )
    }

    pub fn write_to_io(&self, writer: &mut dyn std::io::Write) -> std::io::Result<()> {
        writeln!(
            writer,
            "{level} {time:?} [{tag}] [{file}:{line}:{column} @ {function_name}] {message}",
            level = self.level.short(),
            time = self.timestamp.format("%Y-%m-%d %H:%M:%S"),
            tag = self.tag.as_deref().unwrap_or(DEFAULT_TAG),
            message = self.message,
            line = self.line,
            column = self.column,
            file = self.file,
            function_name = self.function_name.as_deref().unwrap_or("default")
        )
    }
    pub fn write_compact_to_io(&self, writer: &mut dyn std::io::Write) -> std::io::Result<()> {
        writeln!(
            writer,
            "{level} {time:?} [{file}:{line}:{column} @ {function_name}] {message}",
            level = self.level.short(),
            time = self.timestamp.format("%Y-%m-%d %H:%M:%S"),
            message = self.message,
            line = self.line,
            column = self.column,
            file = self.file,
            function_name = self.function_name.as_deref().unwrap_or("default")
        )
    }
}
impl Default for LogData {
    fn default() -> Self {
        Self {
            level: LogLevel::Info,
            tag: None,
            message: String::new(),
            timestamp: Utc::now(),
            file: String::new(),
            line: 0,
            column: 0,
            function_name: None,
        }
    }
}
