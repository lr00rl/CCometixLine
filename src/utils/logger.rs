use chrono::Local;
use std::fs::{File, OpenOptions};
use std::io::Write;
use std::path::Path;
use std::sync::Mutex;

static LOGGER: Mutex<Option<FileLogger>> = Mutex::new(None);

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum LogLevel {
    Debug,
    Info,
    Error,
}

impl LogLevel {
    fn from_str(s: &str) -> Self {
        match s.to_lowercase().as_str() {
            "debug" => LogLevel::Debug,
            "error" => LogLevel::Error,
            _ => LogLevel::Info,
        }
    }

    fn label(&self) -> &'static str {
        match self {
            LogLevel::Debug => "DEBUG",
            LogLevel::Info => "INFO ",
            LogLevel::Error => "ERROR",
        }
    }
}

pub struct FileLogger {
    file: File,
    level: LogLevel,
}

impl FileLogger {
    /// Initialize the global logger.
    /// logto: path to a file or directory.
    /// loglevel: "debug" | "info" | "error"
    pub fn init(logto: &str, loglevel: &str) -> Result<(), Box<dyn std::error::Error>> {
        let level = LogLevel::from_str(loglevel);

        let path = Path::new(logto);
        let file_path = if path.is_dir() || logto.ends_with('/') || logto.ends_with(std::path::MAIN_SEPARATOR) {
            // Directory: create dated log file
            let date = Local::now().format("%Y-%m-%d").to_string();
            let dir_path = path;
            std::fs::create_dir_all(dir_path)?;
            dir_path.join(format!("ccline_{}.log", date))
        } else {
            // File path: create parent dirs if needed
            if let Some(parent) = path.parent() {
                if !parent.as_os_str().is_empty() {
                    std::fs::create_dir_all(parent)?;
                }
            }
            path.to_path_buf()
        };

        let file = OpenOptions::new()
            .create(true)
            .append(true)
            .open(&file_path)?;

        let logger = FileLogger { file, level };

        if let Ok(mut guard) = LOGGER.lock() {
            *guard = Some(logger);
        }

        Ok(())
    }

    fn write_log(level: LogLevel, msg: &str) {
        if let Ok(mut guard) = LOGGER.lock() {
            if let Some(ref mut logger) = *guard {
                if level >= logger.level {
                    let now = Local::now().format("%Y-%m-%d %H:%M:%S%.3f");
                    let line = format!("[{}] [{}] {}\n", now, level.label(), msg);
                    let _ = logger.file.write_all(line.as_bytes());
                    let _ = logger.file.flush();
                }
            }
        }
    }
}

/// Log at DEBUG level. No-op if logger not initialized or level too high.
pub fn debug(msg: &str) {
    FileLogger::write_log(LogLevel::Debug, msg);
}

/// Log at INFO level.
pub fn info(msg: &str) {
    FileLogger::write_log(LogLevel::Info, msg);
}

/// Log at ERROR level.
pub fn error(msg: &str) {
    FileLogger::write_log(LogLevel::Error, msg);
}

/// Macro-like convenience - logs with format string at INFO level.
#[macro_export]
macro_rules! log_info {
    ($($arg:tt)*) => {
        $crate::utils::logger::info(&format!($($arg)*));
    };
}

/// Logs with format string at DEBUG level.
#[macro_export]
macro_rules! log_debug {
    ($($arg:tt)*) => {
        $crate::utils::logger::debug(&format!($($arg)*));
    };
}

/// Logs with format string at ERROR level.
#[macro_export]
macro_rules! log_error {
    ($($arg:tt)*) => {
        $crate::utils::logger::error(&format!($($arg)*));
    };
}
