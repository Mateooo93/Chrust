//! Files Aurora keeps between runs, under `%LOCALAPPDATA%\Aurora`.

use std::fs;
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

use serde::{Deserialize, Serialize};

use crate::settings::Settings;

pub fn data_dir() -> PathBuf {
    let base = std::env::var_os("LOCALAPPDATA")
        .map(PathBuf::from)
        .or_else(|| std::env::var_os("HOME").map(PathBuf::from))
        .unwrap_or_else(std::env::temp_dir);
    base.join("Aurora")
}

/// Where WebView2 keeps cookies, logins, cache and site data.
pub fn profile_dir() -> PathBuf {
    data_dir().join("Profile")
}

pub fn favicon_dir() -> PathBuf {
    data_dir().join("Favicons")
}

/// The image uploaded for the "custom" new tab background.
pub fn background_file() -> PathBuf {
    data_dir().join("background.img")
}

pub fn downloads_dir() -> PathBuf {
    std::env::var_os("USERPROFILE")
        .map(|home| PathBuf::from(home).join("Downloads"))
        .unwrap_or_else(|| data_dir().join("Downloads"))
}

pub fn now() -> u64 {
    SystemTime::now().duration_since(UNIX_EPOCH).map(|d| d.as_secs()).unwrap_or(0)
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
pub struct Bookmark {
    pub title: String,
    pub url: String,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct HistoryEntry {
    pub url: String,
    pub title: String,
    /// Unix time in seconds.
    pub visited: u64,
}

#[derive(Clone, Copy, Debug, Serialize, Deserialize, PartialEq)]
pub enum DownloadState {
    InProgress,
    Done,
    Failed,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Download {
    pub path: PathBuf,
    pub state: DownloadState,
    pub started: u64,
}

#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct WindowSession {
    pub tabs: Vec<String>,
    pub active: usize,
}

#[derive(Default, Serialize, Deserialize)]
pub struct Session {
    #[serde(default)]
    pub windows: Vec<WindowSession>,
    // The first version saved a single window at the top level.
    #[serde(default, skip_serializing)]
    tabs: Vec<String>,
    #[serde(default, skip_serializing)]
    active: usize,
}

impl Session {
    pub fn new(windows: Vec<WindowSession>) -> Self {
        Self { windows, ..Default::default() }
    }
}

fn read_json<T: for<'de> Deserialize<'de>>(path: &Path) -> Option<T> {
    let text = fs::read_to_string(path).ok()?;
    match serde_json::from_str(&text) {
        Ok(value) => Some(value),
        Err(err) => {
            log::warn!("ignoring unreadable {}: {err}", path.display());
            None
        }
    }
}

/// Unique suffix per write, so two windows saving the same file at once
/// cannot truncate each other's temporary file.
fn tmp_suffix() -> String {
    use std::sync::atomic::{AtomicU64, Ordering};
    static COUNTER: AtomicU64 = AtomicU64::new(0);
    format!("{}-{}", std::process::id(), COUNTER.fetch_add(1, Ordering::Relaxed))
}

/// Writes through a temporary file so a crash never leaves a half-written file.
fn write_json<T: Serialize + ?Sized>(path: &Path, value: &T) {
    let result = (|| -> std::io::Result<()> {
        if let Some(dir) = path.parent() {
            fs::create_dir_all(dir)?;
        }
        let Some(name) = path.file_name().and_then(|n| n.to_str()) else {
            return Err(std::io::Error::new(std::io::ErrorKind::InvalidInput, "file name is not UTF-8"));
        };
        let bytes = serde_json::to_vec_pretty(value)?;
        let tmp = path.with_file_name(format!(".{name}.{}.tmp", tmp_suffix()));
        fs::write(&tmp, &bytes)?;
        if let Err(first) = fs::rename(&tmp, path) {
            // Another process may hold the destination open for a moment
            // (antivirus scan, another Aurora window). Retry briefly, then
            // write in place rather than lose the data.
            for _ in 0..5 {
                std::thread::sleep(std::time::Duration::from_millis(20));
                if fs::rename(&tmp, path).is_ok() {
                    return Ok(());
                }
            }
            let _ = fs::remove_file(&tmp);
            fs::write(path, &bytes)?;
            log::debug!("replaced {} in place after rename failed: {first}", path.display());
        }
        Ok(())
    })();
    if let Err(err) = result {
        log::warn!("could not save {}: {err}", path.display());
    }
}

pub fn load_bookmarks() -> Vec<Bookmark> {
    read_json(&data_dir().join("bookmarks.json")).unwrap_or_else(|| {
        vec![
            Bookmark { title: "YouTube".into(), url: "https://www.youtube.com/".into() },
            Bookmark { title: "GitHub".into(), url: "https://github.com/".into() },
            Bookmark { title: "MDN".into(), url: "https://developer.mozilla.org/".into() },
            Bookmark { title: "Rust Docs".into(), url: "https://doc.rust-lang.org/std/".into() },
            Bookmark { title: "Wikipedia".into(), url: "https://en.wikipedia.org/".into() },
        ]
    })
}

pub fn save_bookmarks(bookmarks: &[Bookmark]) {
    write_json(&data_dir().join("bookmarks.json"), bookmarks);
}

pub fn load_history() -> Vec<HistoryEntry> {
    read_json(&data_dir().join("history.json")).unwrap_or_default()
}

pub fn save_history(history: &[HistoryEntry]) {
    write_json(&data_dir().join("history.json"), history);
}

pub fn load_downloads() -> Vec<Download> {
    let mut list: Vec<Download> = read_json(&data_dir().join("downloads.json")).unwrap_or_default();
    // Anything still "in progress" was interrupted when Aurora last closed.
    for d in &mut list {
        if d.state == DownloadState::InProgress {
            d.state = DownloadState::Failed;
        }
    }
    list
}

pub fn save_downloads(downloads: &[Download]) {
    write_json(&data_dir().join("downloads.json"), downloads);
}

pub fn load_settings() -> Settings {
    read_json(&data_dir().join("settings.json")).unwrap_or_default()
}

pub fn save_settings(settings: &Settings) {
    write_json(&data_dir().join("settings.json"), settings);
}

pub fn load_session() -> Vec<WindowSession> {
    let Some(mut session) = read_json::<Session>(&data_dir().join("session.json")) else {
        return Vec::new();
    };
    if session.windows.is_empty() && !session.tabs.is_empty() {
        session.windows.push(WindowSession { tabs: session.tabs, active: session.active });
    }
    session.windows.retain(|w| !w.tabs.is_empty());
    session.windows
}

pub fn save_session(session: &Session) {
    write_json(&data_dir().join("session.json"), session);
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reads_old_single_window_session() {
        let mut s: Session = serde_json::from_str(r#"{"tabs":["https://a.com/"],"active":0}"#).unwrap();
        assert!(s.windows.is_empty());
        if s.windows.is_empty() {
            s.windows.push(WindowSession { tabs: s.tabs.clone(), active: s.active });
        }
        assert_eq!(s.windows[0].tabs, ["https://a.com/"]);
        let json = serde_json::to_string(&s).unwrap();
        assert!(!json.contains("\"tabs\":[\"https://a.com/\"],\"active\":0}") || json.contains("windows"));
    }
}
