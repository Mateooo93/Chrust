//! Files Aurora keeps between runs, under `%LOCALAPPDATA%\Aurora`.

use std::fs;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

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

pub fn downloads_dir() -> PathBuf {
    std::env::var_os("USERPROFILE")
        .map(|home| PathBuf::from(home).join("Downloads"))
        .unwrap_or_else(|| data_dir().join("Downloads"))
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
pub struct Bookmark {
    pub title: String,
    pub url: String,
}

#[derive(Default, Serialize, Deserialize)]
pub struct Session {
    pub tabs: Vec<String>,
    pub active: usize,
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

/// Writes through a temporary file so a crash never leaves a half-written file.
fn write_json<T: Serialize>(path: &Path, value: &T) {
    let result = (|| -> std::io::Result<()> {
        if let Some(dir) = path.parent() {
            fs::create_dir_all(dir)?;
        }
        let tmp = path.with_extension("json.tmp");
        fs::write(&tmp, serde_json::to_vec_pretty(value)?)?;
        fs::rename(&tmp, path)
    })();
    if let Err(err) = result {
        log::warn!("could not save {}: {err}", path.display());
    }
}

pub fn load_bookmarks() -> Vec<Bookmark> {
    read_json(&data_dir().join("bookmarks.json")).unwrap_or_else(|| {
        vec![
            Bookmark { title: "YouTube".into(), url: "https://www.youtube.com/".into() },
            Bookmark { title: "Wikipedia".into(), url: "https://en.wikipedia.org/".into() },
            Bookmark { title: "GitHub".into(), url: "https://github.com/".into() },
        ]
    })
}

pub fn save_bookmarks(bookmarks: &[Bookmark]) {
    write_json(&data_dir().join("bookmarks.json"), &bookmarks);
}

pub fn load_session() -> Option<Session> {
    read_json::<Session>(&data_dir().join("session.json")).filter(|s| !s.tabs.is_empty())
}

pub fn save_session(session: &Session) {
    write_json(&data_dir().join("session.json"), session);
}
