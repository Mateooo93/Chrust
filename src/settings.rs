//! User settings, saved to `settings.json`.

use serde::{Deserialize, Serialize};
use serde_json::Value;

pub struct SearchEngine {
    pub id: &'static str,
    pub name: &'static str,
    /// `{}` is replaced by the URL-encoded query.
    pub url: &'static str,
}

pub const SEARCH_ENGINES: &[SearchEngine] = &[
    SearchEngine { id: "google", name: "Google", url: "https://www.google.com/search?q={}" },
    SearchEngine { id: "duckduckgo", name: "DuckDuckGo", url: "https://duckduckgo.com/?q={}" },
    SearchEngine { id: "bing", name: "Bing", url: "https://www.bing.com/search?q={}" },
    SearchEngine { id: "brave", name: "Brave Search", url: "https://search.brave.com/search?q={}" },
    SearchEngine { id: "startpage", name: "Startpage", url: "https://www.startpage.com/do/search?query={}" },
    SearchEngine { id: "ecosia", name: "Ecosia", url: "https://www.ecosia.org/search?q={}" },
];

pub fn search_engine(id: &str) -> &'static SearchEngine {
    SEARCH_ENGINES.iter().find(|e| e.id == id).unwrap_or(&SEARCH_ENGINES[0])
}

pub const BACKGROUNDS: &[(&str, &str)] = &[
    ("chrustport", "Chrust (animated)"),
    ("nebula", "Nebula (animated)"),
    ("grid", "Dev grid"),
    ("sunset", "Sunset"),
    ("ocean", "Ocean"),
    ("forest", "Forest"),
    ("solid", "Plain"),
    ("custom", "Your image"),
];

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
#[serde(default)]
pub struct Settings {
    /// A palette id from `themes::PALETTES`, or "system".
    pub theme: String,
    /// An id from `BACKGROUNDS`.
    pub background: String,
    /// Image URL for the "custom" background. Empty means the uploaded file.
    pub background_url: String,
    pub search_engine: String,
    pub show_bookmarks_bar: bool,
    pub restore_session: bool,
    pub show_clock: bool,
    /// Fetch site icons from Google's favicon service.
    pub site_icons: bool,
    /// Let WebView2 offer to save and fill passwords.
    pub save_passwords: bool,
    pub show_dev_tools_button: bool,
    pub smooth_animations: bool,
    /// Stop requests the ad blocker's rules match. Private windows always
    /// block, whatever this says (see `adblock`).
    pub adblock: bool,
    /// Download EasyList and EasyPrivacy and add them to the bundled rules.
    pub adblock_upstream: bool,
}

impl Default for Settings {
    fn default() -> Self {
        Self {
            theme: "system".into(),
            background: "chrustport".into(),
            background_url: String::new(),
            search_engine: "google".into(),
            show_bookmarks_bar: true,
            restore_session: true,
            show_clock: true,
            site_icons: true,
            save_passwords: true,
            show_dev_tools_button: true,
            smooth_animations: true,
            adblock: true,
            adblock_upstream: false,
        }
    }
}

impl Settings {
    /// Applies one change sent by the settings page. Unknown keys and values
    /// of the wrong type are rejected, so the page cannot corrupt the file.
    pub fn set(&mut self, key: &str, value: &Value) -> bool {
        fn text(v: &Value, allowed: impl Fn(&str) -> bool) -> Option<String> {
            v.as_str().filter(|s| allowed(s)).map(str::to_string)
        }
        let flag = |v: &Value| v.as_bool();
        match key {
            "theme" => {
                let ok = |s: &str| s == "system" || crate::themes::PALETTES.iter().any(|p| p.id == s);
                text(value, ok).map(|v| self.theme = v).is_some()
            }
            "background" => {
                let ok = |s: &str| BACKGROUNDS.iter().any(|(id, _)| *id == s);
                text(value, ok).map(|v| self.background = v).is_some()
            }
            "background_url" => {
                let ok = |s: &str| s.is_empty() || s.starts_with("https://") || s.starts_with("http://");
                text(value, ok).map(|v| self.background_url = v).is_some()
            }
            "search_engine" => {
                let ok = |s: &str| SEARCH_ENGINES.iter().any(|e| e.id == s);
                text(value, ok).map(|v| self.search_engine = v).is_some()
            }
            "show_bookmarks_bar" => flag(value).map(|v| self.show_bookmarks_bar = v).is_some(),
            "restore_session" => flag(value).map(|v| self.restore_session = v).is_some(),
            "show_clock" => flag(value).map(|v| self.show_clock = v).is_some(),
            "site_icons" => flag(value).map(|v| self.site_icons = v).is_some(),
            "save_passwords" => flag(value).map(|v| self.save_passwords = v).is_some(),
            "show_dev_tools_button" => flag(value).map(|v| self.show_dev_tools_button = v).is_some(),
            "smooth_animations" => flag(value).map(|v| self.smooth_animations = v).is_some(),
            "adblock" => flag(value).map(|v| self.adblock = v).is_some(),
            "adblock_upstream" => flag(value).map(|v| self.adblock_upstream = v).is_some(),
            _ => false,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn set_validates() {
        let mut s = Settings::default();
        assert!(s.set("theme", &json!("dracula")));
        assert_eq!(s.theme, "dracula");
        assert!(!s.set("theme", &json!("<script>")));
        assert!(!s.set("theme", &json!(3)));
        assert!(s.set("show_clock", &json!(false)));
        assert!(!s.show_clock);
        assert!(!s.set("background_url", &json!("javascript:alert(1)")));
        assert!(!s.set("nope", &json!(true)));
    }

    #[test]
    fn old_files_get_defaults() {
        let s: Settings = serde_json::from_str(r#"{"theme":"nord"}"#).unwrap();
        assert_eq!(s.theme, "nord");
        assert!(s.show_bookmarks_bar);
    }
}
