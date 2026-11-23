//! State shared by every browser window: settings, bookmarks, history,
//! downloads, site icons and the saved session.

use std::cell::RefCell;
use std::collections::{BTreeMap, HashMap};
use std::path::PathBuf;
use std::rc::Rc;
use std::time::{Duration, Instant};

use gpui_kit::{App, Global};

use crate::settings::Settings;
use crate::storage::{self, Bookmark, Download, DownloadState, HistoryEntry, Session, WindowSession};
use crate::themes::{self, Palette};

const HISTORY_LIMIT: usize = 5000;
const DOWNLOADS_LIMIT: usize = 200;
/// History and session saves are coalesced: changes mark the file dirty and
/// a timer writes it at most this often, so busy pages and tab switches do
/// not serialize and rewrite whole files on every browser event.
const FLUSH_DELAY: Duration = Duration::from_millis(750);

pub enum IconState {
    Pending,
    Ready(PathBuf),
    /// The site has no usable icon. The failure is remembered for a while so
    /// pages that never have one do not re-download on every render.
    Missing(Instant),
}

/// How long a site without an icon is trusted to still have none.
pub const MISSING_ICON_TTL: Duration = Duration::from_secs(6 * 60 * 60);

pub struct Library {
    pub settings: Settings,
    /// The palette in use; resolves "system" to light or dark.
    pub palette: &'static Palette,
    pub bookmarks: Vec<Bookmark>,
    pub history: Vec<HistoryEntry>,
    pub downloads: Vec<Download>,
    pub icons: HashMap<String, IconState>,
    sessions: BTreeMap<u64, WindowSession>,
    next_window: u64,
    /// The last result of an update check, shown on the settings page and
    /// dropped from memory once the page has been served.
    pub update_message: Option<String>,
    history_dirty: bool,
    history_dirty_since: Option<Instant>,
    session_dirty: bool,
    session_dirty_since: Option<Instant>,
}

pub type Shared = Rc<RefCell<Library>>;

struct LibraryGlobal(Shared);
impl Global for LibraryGlobal {}

// Also reachable without an `App`, for background work (favicon downloads
// and updates, which report back on the UI thread).
thread_local! {
    static GLOBAL: std::cell::OnceCell<Shared> = const { std::cell::OnceCell::new() };
}

pub fn init(cx: &mut App) -> Shared {
    let settings = storage::load_settings();
    let palette = themes::palette(&settings.theme, cx.window_appearance());
    let lib = Rc::new(RefCell::new(Library {
        settings,
        palette,
        bookmarks: storage::load_bookmarks(),
        history: storage::load_history(),
        downloads: storage::load_downloads(),
        icons: HashMap::new(),
        sessions: BTreeMap::new(),
        next_window: 1,
        update_message: None,
        history_dirty: false,
        history_dirty_since: None,
        session_dirty: false,
        session_dirty_since: None,
    }));
    cx.set_global(LibraryGlobal(lib.clone()));
    GLOBAL.with(|global| {
        let _ = global.set(lib.clone());
    });
    lib
}

pub fn get(cx: &App) -> Shared {
    cx.global::<LibraryGlobal>().0.clone()
}

/// The shared library for code running outside a GPUI context (favicon
/// downloads). Valid once `init` has run.
pub fn shared() -> Shared {
    GLOBAL.with(|global| {
        global
            .get()
            .expect("library is initialised before any background work")
            .clone()
    })
}

/// Re-resolves the theme (for "system", after a Windows light/dark switch)
/// and applies it to the interface.
pub fn apply_theme(cx: &mut App) {
    let lib = get(cx);
    let palette = themes::palette(&lib.borrow().settings.theme, cx.window_appearance());
    lib.borrow_mut().palette = palette;
    themes::apply(palette, cx);
    cx.refresh_windows();
}

impl Library {
    pub fn save_settings(&self) {
        storage::save_settings(&self.settings);
    }

    // ----- coalesced saves ------------------------------------------------

    /// How long until a dirty file is due to be written; None when clean.
    pub fn time_until_flush(&self, now: Instant) -> Option<Duration> {
        [(self.history_dirty, self.history_dirty_since), (self.session_dirty, self.session_dirty_since)]
            .into_iter()
            .filter(|(dirty, _)| *dirty)
            .filter_map(|(_, since)| since.map(|since| FLUSH_DELAY.saturating_sub(now.duration_since(since))))
            .min()
    }

    /// Writes every dirty file. Cheap when nothing changed.
    pub fn flush(&mut self) {
        if self.history_dirty {
            self.history_dirty = false;
            self.history_dirty_since = None;
            storage::save_history(&self.history);
        }
        if self.session_dirty {
            self.session_dirty = false;
            self.session_dirty_since = None;
            self.write_session();
        }
    }

    fn mark_history(&mut self) {
        if !self.history_dirty {
            self.history_dirty = true;
            self.history_dirty_since = Some(Instant::now());
        }
    }

    fn mark_session(&mut self) {
        if !self.session_dirty {
            self.session_dirty = true;
            self.session_dirty_since = Some(Instant::now());
        }
    }

    // ----- bookmarks ------------------------------------------------------

    pub fn is_bookmarked(&self, url: &str) -> bool {
        self.bookmarks.iter().any(|b| b.url == url)
    }

    /// Adds the page, or removes it if it is already bookmarked.
    pub fn toggle_bookmark(&mut self, url: &str, title: &str) {
        if let Some(pos) = self.bookmarks.iter().position(|b| b.url == url) {
            self.bookmarks.remove(pos);
        } else {
            self.bookmarks.push(Bookmark { title: title.to_string(), url: url.to_string() });
        }
        storage::save_bookmarks(&self.bookmarks);
    }

    pub fn remove_bookmark(&mut self, url: &str) {
        self.bookmarks.retain(|b| b.url != url);
        storage::save_bookmarks(&self.bookmarks);
    }

    /// Renames a bookmark and/or changes its address. Returns false if `url`
    /// is not bookmarked.
    pub fn edit_bookmark(&mut self, url: &str, title: &str, new_url: &str) -> bool {
        let Some(b) = self.bookmarks.iter_mut().find(|b| b.url == url) else {
            return false;
        };
        b.title = title.to_string();
        b.url = new_url.to_string();
        storage::save_bookmarks(&self.bookmarks);
        true
    }

    pub fn add_bookmarks(&mut self, new: Vec<Bookmark>) {
        if !new.is_empty() {
            self.bookmarks.extend(new);
            storage::save_bookmarks(&self.bookmarks);
        }
    }

    // ----- history --------------------------------------------------------

    pub fn record_visit(&mut self, url: &str, title: &str) {
        if !(url.starts_with("http://") || url.starts_with("https://")) || crate::address::is_internal(url) {
            return;
        }
        let now = storage::now();
        match self.history.last_mut() {
            // Reloads and redirects to the same page update the last entry.
            Some(last) if last.url == url => {
                last.visited = now;
                if !title.is_empty() {
                    last.title = title.to_string();
                }
            }
            _ => self.history.push(HistoryEntry { url: url.into(), title: title.into(), visited: now }),
        }
        if self.history.len() > HISTORY_LIMIT {
            let extra = self.history.len() - HISTORY_LIMIT;
            self.history.drain(..extra);
        }
        self.mark_history();
    }

    /// Titles often arrive after the load finishes.
    pub fn update_title(&mut self, url: &str, title: &str) {
        if title.is_empty() {
            return;
        }
        if let Some(entry) = self.history.iter_mut().rev().take(20).find(|e| e.url == url) {
            if entry.title != title {
                entry.title = title.to_string();
                self.mark_history();
            }
        }
    }

    pub fn remove_history(&mut self, url: &str, visited: u64) {
        self.history.retain(|e| !(e.url == url && e.visited == visited));
        self.mark_history();
    }

    pub fn clear_history(&mut self) {
        self.history.clear();
        self.mark_history();
    }

    // ----- downloads ------------------------------------------------------

    pub fn download_started(&mut self, path: PathBuf) {
        self.downloads.push(Download { path, state: DownloadState::InProgress, started: storage::now() });
        if self.downloads.len() > DOWNLOADS_LIMIT {
            self.downloads.remove(0);
        }
        storage::save_downloads(&self.downloads);
    }

    pub fn download_finished(&mut self, path: Option<PathBuf>, success: bool) {
        let state = if success { DownloadState::Done } else { DownloadState::Failed };
        let entry = self
            .downloads
            .iter_mut()
            .rev()
            .find(|d| d.state == DownloadState::InProgress && path.as_ref().is_none_or(|p| *p == d.path));
        match entry {
            Some(entry) => {
                entry.state = state;
                if let Some(p) = path {
                    entry.path = p;
                }
            }
            None => {
                if let Some(p) = path {
                    self.downloads.push(Download { path: p, state, started: storage::now() });
                }
            }
        }
        storage::save_downloads(&self.downloads);
    }

    pub fn active_downloads(&self) -> usize {
        self.downloads.iter().filter(|d| d.state == DownloadState::InProgress).count()
    }

    pub fn clear_downloads(&mut self) {
        self.downloads.retain(|d| d.state == DownloadState::InProgress);
        storage::save_downloads(&self.downloads);
    }

    // ----- session --------------------------------------------------------

    /// A slot in the saved session for a new (non-private) window.
    pub fn register_window(&mut self) -> u64 {
        let id = self.next_window;
        self.next_window += 1;
        id
    }

    pub fn update_window(&mut self, id: u64, session: WindowSession) {
        if self.sessions.get(&id) == Some(&session) {
            return;
        }
        self.sessions.insert(id, session);
        self.mark_session();
    }

    /// The user closed a window. If other windows are still in the session it
    /// is dropped; the last one stays so it is restored next time.
    pub fn forget_window_unless_last(&mut self, id: u64) {
        if self.sessions.len() > 1 {
            self.sessions.remove(&id);
            self.mark_session();
        }
    }

    fn write_session(&self) {
        storage::save_session(&Session::new(self.sessions.values().cloned().collect()));
    }
}
