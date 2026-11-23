//! A log file for things that go wrong, since release builds have no console.
//!
//! Everything `log` records and every panic is appended to
//! `%LOCALAPPDATA%\Aurora\chrust.log`, which keeps its last 2 MB. It is
//! written synchronously: a crash is exactly when buffered output is lost.
//! Set `RUST_LOG` to widen what is recorded (`RUST_LOG=debug`).

use std::fs::{File, OpenOptions};
use std::io::Write as _;
use std::path::PathBuf;
use std::sync::Mutex;

struct FileLogger {
    file: Mutex<Option<File>>,
}

impl log::Log for FileLogger {
    fn enabled(&self, _: &log::Metadata) -> bool {
        true
    }

    fn log(&self, record: &log::Record) {
        let line = format!(
            "{} {:<5} {}: {}\n",
            timestamp(),
            record.level(),
            record.target(),
            record.args()
        );
        if let Ok(mut file) = self.file.lock() {
            if let Some(file) = file.as_mut() {
                let _ = file.write_all(line.as_bytes());
            }
        }
    }

    fn flush(&self) {
        if let Ok(mut file) = self.file.lock() {
            if let Some(file) = file.as_mut() {
                let _ = file.flush();
            }
        }
    }
}

/// Seconds since the epoch, as a plain number; no dependency for a format.
fn timestamp() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0)
}

fn level() -> log::LevelFilter {
    match std::env::var("RUST_LOG").unwrap_or_default().to_lowercase().as_str() {
        "trace" => log::LevelFilter::Trace,
        "debug" => log::LevelFilter::Debug,
        "info" => log::LevelFilter::Info,
        "warn" => log::LevelFilter::Warn,
        "error" => log::LevelFilter::Error,
        _ => log::LevelFilter::Info,
    }
}

/// Starts the log; returns the file's path, if it could be opened.
pub fn start() -> Option<PathBuf> {
    let path = crate::storage::data_dir().join("chrust.log");
    if let Some(dir) = path.parent() {
        let _ = std::fs::create_dir_all(dir);
    }
    // A fresh file each start keeps old crashes readable for a while: the
    // previous one is kept beside it as `chrust.log.old`.
    let old = path.with_file_name("chrust.log.old");
    let _ = std::fs::rename(&path, &old);
    let file = OpenOptions::new().create(true).append(true).open(&path).ok();
    let logger = FileLogger { file: Mutex::new(file) };
    let started = logger.file.lock().map(|f| f.is_some()).unwrap_or(false);
    if log::set_boxed_logger(Box::new(logger)).is_ok() {
        log::set_max_level(level());
    }
    started.then_some(path)
}

/// Turns a webview error into something worth putting in a notification.
pub fn describe(err: &anyhow::Error) -> String {
    if let Some(webview2_com::Error::WindowsError(inner)) =
        err.chain().find_map(|cause| cause.downcast_ref::<webview2_com::Error>())
    {
        return inner.to_string();
    }
    err.to_string()
}

/// Whether starting the engine can simply be tried again: these codes mean
/// "the browser process is still coming up" or "a window transition is in
/// progress", both of which clear within a moment.
pub fn worth_retrying(err: &anyhow::Error) -> bool {
    use windows::Win32::Foundation::{
        ERROR_INVALID_STATE, RPC_E_CALL_CANCELED, RPC_E_SERVERCALL_RETRYLATER, RPC_E_WRONG_THREAD,
    };
    err.chain()
        .find_map(|cause| cause.downcast_ref::<webview2_com::Error>())
        .and_then(|err| match err {
            webview2_com::Error::WindowsError(err) => Some(err.code()),
            _ => None,
        })
        .is_some_and(|code| {
            [
                RPC_E_SERVERCALL_RETRYLATER,
                RPC_E_CALL_CANCELED,
                RPC_E_WRONG_THREAD,
                windows::core::HRESULT::from_win32(ERROR_INVALID_STATE.0),
            ]
            .contains(&code)
        })
}
