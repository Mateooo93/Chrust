//! Importing bookmarks from Google Chrome.
//!
//! Chrome stores bookmarks as JSON in each profile folder: `Bookmarks` for
//! local ones and `AccountBookmarks` for ones synced to a Google account.
//! Saved passwords and cookies are encrypted so that only Chrome can read
//! them, so they are not imported.

use std::collections::HashSet;
use std::fs;
use std::path::{Path, PathBuf};

use serde_json::Value;

use crate::storage::Bookmark;

pub struct ImportResult {
    pub added: Vec<Bookmark>,
    /// Bookmarks found in Chrome, including ones Chrust already had.
    pub found: usize,
    /// Display names of the Chrome profiles that were read.
    pub profiles: Vec<String>,
}

fn chrome_user_data() -> Option<PathBuf> {
    #[cfg(windows)]
    {
        let base = std::env::var_os("LOCALAPPDATA")?;
        let dir = PathBuf::from(base).join("Google").join("Chrome").join("User Data");
        return dir.is_dir().then_some(dir);
    }
    // Chrome on Linux: $XDG_CONFIG_HOME/google-chrome (Chromium:
    // ~/.config/chromium). Both are read when present.
    #[cfg(not(windows))]
    {
        let base = std::env::var_os("XDG_CONFIG_HOME")
            .map(PathBuf::from)
            .filter(|p| p.is_absolute())
            .or_else(|| std::env::var_os("HOME").map(|h| PathBuf::from(h).join(".config")))?;
        ["google-chrome", "chromium"]
            .into_iter()
            .map(|name| base.join(name))
            .find(|dir| dir.is_dir())
    }
}

/// (folder name, display name) of every Chrome profile.
fn profiles(user_data: &Path) -> Vec<(String, String)> {
    let mut out = Vec::new();
    if let Some(cache) = fs::read_to_string(user_data.join("Local State"))
        .ok()
        .and_then(|s| serde_json::from_str::<Value>(&s).ok())
        .and_then(|v| v["profile"]["info_cache"].as_object().cloned())
    {
        for (dir, info) in cache {
            let name = info["name"].as_str().unwrap_or(&dir).to_string();
            out.push((dir, name));
        }
    }
    if out.is_empty() && user_data.join("Default").is_dir() {
        out.push(("Default".into(), "Default".into()));
    }
    out.sort();
    out
}

/// Adds every web link under `node` to `out`, depth first, in Chrome's order.
fn collect(node: &Value, out: &mut Vec<Bookmark>) {
    match node["type"].as_str() {
        Some("url") => {
            let url = node["url"].as_str().unwrap_or_default();
            if url.starts_with("https://") || url.starts_with("http://") {
                let title = node["name"].as_str().unwrap_or_default().trim();
                out.push(Bookmark {
                    title: if title.is_empty() { url.to_string() } else { title.to_string() },
                    url: url.to_string(),
                });
            }
        }
        _ => {
            if let Some(children) = node["children"].as_array() {
                for child in children {
                    collect(child, out);
                }
            }
        }
    }
}

/// Reads one Chrome bookmarks file. The bookmarks bar comes first, as in Chrome.
pub fn parse_bookmarks(json: &str) -> Vec<Bookmark> {
    let Ok(root) = serde_json::from_str::<Value>(json) else {
        return Vec::new();
    };
    let mut out = Vec::new();
    for key in ["bookmark_bar", "other", "synced"] {
        collect(&root["roots"][key], &mut out);
    }
    out
}

/// Bookmarks from every Chrome profile that `existing` does not have yet.
pub fn import_chrome(existing: &[Bookmark]) -> Result<ImportResult, String> {
    let user_data = chrome_user_data().ok_or("Google Chrome is not installed for this user.")?;

    let mut seen: HashSet<String> = existing.iter().map(|b| b.url.clone()).collect();
    let mut result = ImportResult { added: Vec::new(), found: 0, profiles: Vec::new() };

    for (dir, name) in profiles(&user_data) {
        let mut any = false;
        for file in ["Bookmarks", "AccountBookmarks"] {
            let Ok(json) = fs::read_to_string(user_data.join(&dir).join(file)) else {
                continue;
            };
            any = true;
            for bookmark in parse_bookmarks(&json) {
                result.found += 1;
                if seen.insert(bookmark.url.clone()) {
                    result.added.push(bookmark);
                }
            }
        }
        if any {
            result.profiles.push(name);
        }
    }

    if result.profiles.is_empty() {
        return Err("No Chrome profile with bookmarks was found.".into());
    }
    Ok(result)
}

#[cfg(test)]
mod tests {
    use super::*;

    const SAMPLE: &str = r#"{
      "roots": {
        "bookmark_bar": { "type": "folder", "children": [
          { "type": "url", "name": "Rust", "url": "https://www.rust-lang.org/" },
          { "type": "folder", "name": "Dev", "children": [
            { "type": "url", "name": "", "url": "https://github.com/" },
            { "type": "url", "name": "Bookmarklet", "url": "javascript:alert(1)" }
          ]}
        ]},
        "other": { "type": "folder", "children": [
          { "type": "url", "name": "Wiki", "url": "https://en.wikipedia.org/" }
        ]},
        "synced": { "type": "folder", "children": [] }
      }
    }"#;

    #[test]
    fn flattens_folders_and_skips_scripts() {
        let got = parse_bookmarks(SAMPLE);
        let urls: Vec<_> = got.iter().map(|b| b.url.as_str()).collect();
        assert_eq!(urls, ["https://www.rust-lang.org/", "https://github.com/", "https://en.wikipedia.org/"]);
        assert_eq!(got[1].title, "https://github.com/");
    }

    #[test]
    fn bad_json_gives_nothing() {
        assert!(parse_bookmarks("not json").is_empty());
    }
}
