//! A browser window: tab strip, toolbar, bookmarks bar, find bar, sidebar
//! panels and the active page.
//!
//! GPUI popups cannot draw over a WebView2 page on Windows, so everything
//! that would usually be a menu or popup here takes its own space instead:
//! the sidebar pushes the page aside and bars push it down.

use std::cell::RefCell;
use std::path::{Path, PathBuf};
use std::rc::Rc;
use std::time::{Duration, Instant};

use base64::Engine as _;
use futures::StreamExt as _;
use futures::channel::mpsc::{UnboundedSender, unbounded};
use gpui_kit::assets::IconName as Icons;
use gpui_kit::component::{
    ActiveTheme as _, Disableable as _, Icon, Selectable as _, Sizable as _, TitleBar,
    button::{Button, ButtonVariants as _},
    h_flex,
    input::{Input, InputEvent, InputState},
    v_flex,
};
use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::*;
use serde_json::Value;
use wry::WebContext;

use crate::library::{self, Shared};
use crate::native::NavigationStatus;
use crate::storage::{self, DownloadState, WindowSession};
use crate::tab::{PageEvent, Shortcut, Tab, TabId};
use crate::{address, favicons, import, native, perf, settings};

actions!(
    aurora,
    [
        NewTab, NewWindow, NewPrivateWindow, CloseTab, ReopenClosedTab, NextTab, PrevTab,
        FocusAddress, Reload, GoBack, GoForward, GoHome, ToggleBookmark, OpenDevTools, Find,
        ShowHistory, ShowDownloads, ShowBookmarks, ShowMenu, OpenSettings, ToggleBookmarksBar,
        ViewSource, CheckForUpdates, ToggleFullscreen, ZoomIn, ZoomOut, ZoomReset, Print, CloseOverlays,
    ]
);

pub fn bind_keys(cx: &mut App) {
    cx.bind_keys([
        KeyBinding::new("ctrl-t", NewTab, None),
        KeyBinding::new("ctrl-n", NewWindow, None),
        KeyBinding::new("ctrl-shift-n", NewPrivateWindow, None),
        KeyBinding::new("ctrl-w", CloseTab, None),
        KeyBinding::new("ctrl-f4", CloseTab, None),
        KeyBinding::new("ctrl-shift-t", ReopenClosedTab, None),
        KeyBinding::new("ctrl-tab", NextTab, None),
        KeyBinding::new("ctrl-pagedown", NextTab, None),
        KeyBinding::new("ctrl-shift-tab", PrevTab, None),
        KeyBinding::new("ctrl-pageup", PrevTab, None),
        KeyBinding::new("ctrl-l", FocusAddress, None),
        KeyBinding::new("alt-d", FocusAddress, None),
        KeyBinding::new("f6", FocusAddress, None),
        KeyBinding::new("f5", Reload, None),
        KeyBinding::new("ctrl-r", Reload, None),
        KeyBinding::new("alt-left", GoBack, None),
        KeyBinding::new("alt-right", GoForward, None),
        KeyBinding::new("alt-home", GoHome, None),
        KeyBinding::new("ctrl-d", ToggleBookmark, None),
        KeyBinding::new("f12", OpenDevTools, None),
        KeyBinding::new("ctrl-shift-i", OpenDevTools, None),
        KeyBinding::new("ctrl-f", Find, None),
        KeyBinding::new("ctrl-h", ShowHistory, None),
        KeyBinding::new("ctrl-j", ShowDownloads, None),
        KeyBinding::new("ctrl-shift-o", ShowBookmarks, None),
        KeyBinding::new("alt-f", ShowMenu, None),
        KeyBinding::new("ctrl-,", OpenSettings, None),
        KeyBinding::new("ctrl-shift-b", ToggleBookmarksBar, None),
        KeyBinding::new("ctrl-u", ViewSource, None),
        KeyBinding::new("f11", ToggleFullscreen, None),
        KeyBinding::new("ctrl-=", ZoomIn, None),
        KeyBinding::new("ctrl-+", ZoomIn, None),
        KeyBinding::new("ctrl--", ZoomOut, None),
        KeyBinding::new("ctrl-0", ZoomReset, None),
        KeyBinding::new("ctrl-p", Print, None),
        KeyBinding::new("escape", CloseOverlays, None),
    ]);
}

/// Opens a browser window. Empty `urls` shows the new tab page.
pub fn open_window(urls: Vec<String>, active: usize, private: bool, cx: &mut App) {
    let mut first = Some((urls, active));
    let options = WindowOptions {
        window_bounds: Some(WindowBounds::centered(size(px(1280.), px(840.)), cx)),
        window_min_size: Some(size(px(520.), px(360.))),
        // Shown without taking focus: the window comes up with its frame
        // while WebView2 starts, instead of the user waiting on a blank space.
        focus: false,
        ..TitleBar::window_options()
    };
    // Creating a page inside the window closure blocks on starting WebView2's
    // browser process, before the window is on screen. The first tabs are
    // therefore opened by the browser itself, in its first render.
    let opened = gpui_kit::open_window(options, cx, |window, cx| {
        cx.new(|cx| Browser::new(first.take(), private, window, cx))
    });
    if let Err(err) = opened {
        log::error!("could not open a window: {err:#}");
    }
}

const ZOOM_LEVELS: &[f64] = &[0.25, 0.33, 0.5, 0.67, 0.75, 0.8, 0.9, 1.0, 1.1, 1.25, 1.5, 1.75, 2.0, 2.5, 3.0, 4.0, 5.0];
const SIDEBAR_W: f32 = 340.;
const PRIVATE_COLOR: u32 = 0xa855f7;

#[derive(Clone, Copy, PartialEq, Debug)]
enum Panel {
    Menu,
    Bookmarks,
    History,
    Downloads,
}

/// Everything the browser can be asked to do, from buttons, keys or pages.
#[derive(Clone, Copy, PartialEq, Debug)]
enum Cmd {
    NewTab,
    NewWindow,
    NewPrivateWindow,
    CloseTab,
    ReopenClosedTab,
    NextTab,
    PrevTab,
    SelectTab(usize),
    FocusAddress,
    ToggleBookmark,
    Find,
    Panel(Panel),
    Settings,
    ToggleBookmarksBar,
    ViewSource,
    CheckForUpdates,
    Fullscreen,
    Reload,
    Stop,
    Back,
    Forward,
    Home,
    DevTools,
    ZoomIn,
    ZoomOut,
    ZoomReset,
    Print,
    CopyUrl,
    ImportChrome,
    CloseOverlays,
}

impl From<Shortcut> for Cmd {
    fn from(s: Shortcut) -> Self {
        match s {
            Shortcut::NewTab => Cmd::NewTab,
            Shortcut::NewWindow => Cmd::NewWindow,
            Shortcut::NewPrivateWindow => Cmd::NewPrivateWindow,
            Shortcut::CloseTab => Cmd::CloseTab,
            Shortcut::ReopenClosedTab => Cmd::ReopenClosedTab,
            Shortcut::NextTab => Cmd::NextTab,
            Shortcut::PrevTab => Cmd::PrevTab,
            Shortcut::SelectTab(n) => Cmd::SelectTab(n),
            Shortcut::FocusAddress => Cmd::FocusAddress,
            Shortcut::ToggleBookmark => Cmd::ToggleBookmark,
            Shortcut::Find => Cmd::Find,
            Shortcut::History => Cmd::Panel(Panel::History),
            Shortcut::Downloads => Cmd::Panel(Panel::Downloads),
            Shortcut::Bookmarks => Cmd::Panel(Panel::Bookmarks),
            Shortcut::Settings => Cmd::Settings,
            Shortcut::ToggleBookmarksBar => Cmd::ToggleBookmarksBar,
            Shortcut::ViewSource => Cmd::ViewSource,
            Shortcut::CheckForUpdates => Cmd::CheckForUpdates,
            Shortcut::Fullscreen => Cmd::Fullscreen,
        }
    }
}

/// A one-line notice under the toolbar.
struct Notice {
    icon: Icons,
    text: String,
    folder: Option<PathBuf>,
}

pub struct Browser {
    focus_handle: FocusHandle,
    private: bool,
    /// Slot in the saved session; `None` for private windows.
    session_id: Option<u64>,
    tabs: Vec<Tab>,
    active: usize,
    next_id: TabId,
    closed: Vec<String>,
    address: Entity<InputState>,
    find_input: Entity<InputState>,
    history_search: Entity<InputState>,
    web_context: WebContext,
    lib: Shared,
    events: UnboundedSender<(TabId, PageEvent)>,
    notice: Option<Notice>,
    sidebar: Option<Panel>,
    /// Bumped each time the sidebar opens, so its slide-in animation replays.
    sidebar_opened: usize,
    find_open: bool,
    find_count: Option<usize>,
    /// A page element (video) is fullscreen.
    page_fullscreen: bool,
    /// F11 fullscreen, which hides the browser chrome.
    immersive: bool,
    /// The tabs to open on the first render; see `restore`.
    pending: Option<(Vec<String>, usize)>,
    /// The title the window last showed, so page events that do not change it
    /// do not talk to the platform.
    window_title: String,
    _subscriptions: Vec<Subscription>,
}

impl Browser {
    fn new(first: Option<(Vec<String>, usize)>, private: bool, window: &mut Window, cx: &mut Context<Self>) -> Self {
        let lib = library::get(cx);
        let address = cx.new(|cx| InputState::new(window, cx).placeholder("Search or type a URL"));
        let find_input = cx.new(|cx| InputState::new(window, cx).placeholder("Find in page"));
        let history_search = cx.new(|cx| InputState::new(window, cx).placeholder("Search history"));
        let subscriptions = vec![
            cx.subscribe_in(&address, window, Self::on_address_event),
            cx.subscribe_in(&find_input, window, Self::on_find_event),
            cx.subscribe_in(&history_search, window, |_, _, _: &InputEvent, _, cx| cx.notify()),
            cx.observe_window_appearance(window, |_, _, cx| library::apply_theme(cx)),
        ];

        let (events, mut rx) = unbounded::<(TabId, PageEvent)>();
        cx.spawn_in(window, async move |this, cx| {
            while let Some((id, event)) = rx.next().await {
                // The window can be busy in a frame, or starting a page, for a
                // moment; an update fails then. The event is kept and tried
                // again instead of being dropped — or, worse, ending this loop
                // and with it every later page event.
                let pending = Rc::new(RefCell::new(Some(event)));
                let mut attempts = 0;
                loop {
                    let event = pending.clone();
                    let done = this.update_in(cx, move |this, window, cx| {
                        if let Some(event) = event.borrow_mut().take() {
                            this.on_page_event(id, event, window, cx);
                        }
                    });
                    match done {
                        Ok(()) => break,
                        Err(_) if attempts < 500 => {
                            attempts += 1;
                            cx.background_executor().timer(Duration::from_millis(4)).await;
                        }
                        Err(_) => break,
                    }
                }
            }
        })
        .detach();

        let session_id = (!private).then(|| lib.borrow_mut().register_window());
        if let Some(id) = session_id {
            let lib = lib.clone();
            let flush = lib.clone();
            window.on_window_should_close(cx, move |_, _| {
                // Closing one of several windows drops it from the session;
                // closing the last one keeps it, so it comes back next time.
                lib.borrow_mut().forget_window_unless_last(id);
                flush.borrow_mut().flush();
                true
            });
        }

        let mut this = Self {
            focus_handle: cx.focus_handle(),
            private,
            session_id,
            tabs: Vec::new(),
            active: 0,
            next_id: 1,
            closed: Vec::new(),
            address,
            find_input,
            history_search,
            web_context: WebContext::new(Some(storage::profile_dir())),
            lib,
            events,
            notice: None,
            sidebar: None,
            sidebar_opened: 0,
            find_open: false,
            find_count: None,
            page_fullscreen: false,
            immersive: false,
            pending: None,
            window_title: String::new(),
            _subscriptions: subscriptions,
        };
        this.pending = first;
        perf::mark_once("window built");
        this
    }

    /// Opens the tabs a window starts with, on the window's first frame: by
    /// then the window is on screen, so the wait for WebView2 is not a blank
    /// or frozen window.
    fn restore(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some((urls, active)) = self.pending.take() else { return };
        let urls = if urls.is_empty() { vec![address::NEW_TAB_URL.to_string()] } else { urls };
        let active = active.min(urls.len() - 1);
        // Each page goes back in its own place. Inserting them after the
        // active tab (as a new tab does) would shuffle a restored session on
        // every start.
        for (i, url) in urls.iter().enumerate() {
            self.open_tab_at(url, i, i == active, window, cx);
        }
        perf::mark("session restored");
        self.update_window_title(window);
    }

    // ----- helpers --------------------------------------------------------

    fn active_tab(&self) -> Option<&Tab> {
        self.tabs.get(self.active)
    }

    fn tab_index(&self, id: TabId) -> Option<usize> {
        self.tabs.iter().position(|t| t.id == id)
    }

    fn raw<R>(&self, cx: &App, f: impl FnOnce(&wry::WebView) -> R) -> Option<R> {
        self.active_tab().map(|tab| f(tab.raw(cx)))
    }

    fn active_url(&self) -> String {
        self.active_tab().map(|t| t.url.clone()).unwrap_or_default()
    }

    fn search_template(&self) -> &'static str {
        settings::search_engine(&self.lib.borrow().settings.search_engine).url
    }

    /// Writes history and the session once their coalescing delay has passed.
    /// Called where those change and while frames are drawn, so nothing is
    /// saved from a timer that could fire in the middle of a frame.
    fn flush_if_due(&self) {
        self.lib.borrow_mut().flush_if_due(Instant::now());
    }

    fn animate(&self) -> bool {
        self.lib.borrow().settings.smooth_animations
    }

    /// Hands keyboard focus from the native page back to GPUI.
    fn take_focus_from_page(&self, cx: &App) {
        self.raw(cx, |w| {
            let _ = w.focus_parent();
        });
    }

    /// Sends the current bookmark list to an open new tab page, so an edit
    /// shows there without reloading it (and lost focus).
    fn mirror_bookmarks(&self, tab: TabId, cx: &App) {
        let Some(i) = self.tab_index(tab) else { return };
        if !address::is_new_tab(&self.tabs[i].url) {
            return;
        }
        let list: Vec<_> = self
            .lib
            .borrow()
            .bookmarks
            .iter()
            .filter(|b| b.url.starts_with("https://") || b.url.starts_with("http://"))
            .take(6)
            .map(|b| serde_json::json!({ "title": b.title, "url": b.url }))
            .collect();
        let list = serde_json::to_string(&list).unwrap_or_else(|_| "[]".into());
        let _ = self.tabs[i]
            .raw(cx)
            .evaluate_script(&format!("window.__auroraBookmarks && window.__auroraBookmarks({list})"));
    }

    /// Downloads a Chrome Web Store extension and hands it to the engine.
    /// The network part runs on its own thread; progress is shown on the
    /// settings page that asked.
    fn install_extension(&self, input: &str, tab: TabId, cx: &mut Context<Self>) {
        let Some(id) = crate::extensions::id_from_input(input) else {
            self.notify_page(tab, "That is not a Chrome Web Store link or extension id.", cx);
            return;
        };
        #[cfg(not(windows))]
        {
            let _ = id;
            self.notify_page(tab, "Extensions need the WebView2 engine, which only runs on Windows.", cx);
            return;
        }
        #[cfg(windows)]
        {
            self.notify_page(tab, "Downloading the extension…", cx);
            let (tx, rx) = futures::channel::oneshot::channel();
            std::thread::spawn(move || {
                let _ = tx.send(crate::extensions::install(&id));
            });
            // The list on the settings page only shows extensions at start,
            // so the page reloads once the result is in.
            cx.spawn(async move |this, cx| {
                let text = match rx.await.ok() {
                    Some(Ok(name)) => format!("Extension installed: {name}"),
                    Some(Err(err)) => format!("Could not install the extension: {err}"),
                    None => "The extension download did not finish.".to_string(),
                };
                let _ = this.update_in(cx, |this, _, cx| {
                    // Keep the result to show on the settings page, which
                    // reloads to list the new extension.
                    let settings = this.tab_index(tab).is_some_and(|i| {
                        address::internal_page(&this.tabs[i].url) == Some("settings")
                    });
                    if settings {
                        // The page shows it after reloading with the list.
                        this.lib.borrow_mut().update_message = Some(text);
                        if let Some(i) = this.tab_index(tab) {
                            let _ = this.tabs[i].raw(cx).reload();
                        }
                    } else {
                        this.show_notice(Icons::Box, text, cx);
                    }
                });
            })
            .detach();
        }
    }

    fn notify_page(&self, tab: TabId, message: &str, cx: &App) {
        if let Some(i) = self.tab_index(tab) {
            let msg = serde_json::to_string(message).unwrap_or_default();
            let _ = self.tabs[i]
                .raw(cx)
                .evaluate_script(&format!("window.__auroraToast && window.__auroraToast({msg})"));
        }
    }

    fn show_notice(&mut self, icon: Icons, text: impl Into<String>, cx: &mut Context<Self>) {
        self.notice = Some(Notice { icon, text: text.into(), folder: None });
        cx.notify();
    }

    // ----- tabs -----------------------------------------------------------

    fn open_tab(&mut self, url: &str, activate: bool, window: &mut Window, cx: &mut Context<Self>) {
        let index = if self.tabs.is_empty() { 0 } else { (self.active + 1).min(self.tabs.len()) };
        self.open_tab_at(url, index, activate, window, cx);
    }

    /// Opens a page at `index` in the tab strip. `open_tab` puts new tabs after
    /// the active one; restoring a session places each page where it was.
    fn open_tab_at(
        &mut self,
        url: &str,
        index: usize,
        activate: bool,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let id = self.next_id;
        self.next_id += 1;
        let tab = Tab::new(
            id,
            url,
            self.private,
            activate,
            &mut self.web_context,
            self.lib.clone(),
            self.events.clone(),
            window,
            cx,
        );
        let tab = match tab {
            Ok(tab) => tab,
            Err(err) => {
                log::error!("could not create a WebView2 page: {err:#}");
                // If this browser shares one engine environment and a page
                // would not start with it, it is given up on and the attempt is
                // made again the old way: one engine per page.
                if crate::engine::refuse() {
                    log::warn!("the shared page engine would not start a page; starting each page its own from now on");
                    self.open_tab_at(url, index, activate, window, cx);
                    return;
                }
                // The engine is busy for a moment while another tab closes
                // or a window changes, and says so; trying again shortly is
                // what other engine embedders do.
                if crate::crashlog::worth_retrying(&err) {
                    log::info!("new tab: engine busy, retrying");
                    let retry_url = url.to_string();
                    let retry_at = index;
                    cx.spawn_in(window, async move |this, cx| {
                        cx.background_executor().timer(Duration::from_millis(350)).await;
                        let _ = this.update_in(cx, |this, window, cx| {
                            this.open_tab_at(&retry_url, retry_at, activate, window, cx);
                        });
                    })
                    .detach();
                    return;
                }
                self.show_notice(
                    Icons::TriangleAlert,
                    format!(
                        "Could not open a new tab: {}. Details are in chrust.log in your data folder.",
                        crate::crashlog::describe(&err)
                    ),
                    cx,
                );
                return;
            }
        };

        perf::mark(format!("tab {id} ready"));
        let index = index.min(self.tabs.len());
        let mut tab = tab;
        if !activate {
            // Hidden pages must not hold keyboard focus and only need
            // minimal memory until they are shown.
            tab.webview.update(cx, |w, _| {
                let _ = w.raw().focus_parent();
                if w.visible() {
                    w.hide();
                }
            });
            set_memory_level(&mut tab, false, cx);
        }
        self.tabs.insert(index, tab);
        if activate {
            self.activate(index, window, cx);
            if address::is_new_tab(url) {
                self.focus_address(window, cx);
            }
        } else if index <= self.active && self.tabs.len() > 1 {
            self.active += 1;
        }
        self.save_session();
        self.flush_if_due();
        cx.notify();
    }

    fn activate(&mut self, index: usize, window: &mut Window, cx: &mut Context<Self>) {
        if index >= self.tabs.len() {
            return;
        }
        if self.page_fullscreen {
            self.set_page_fullscreen(false, window);
        }
        let previous = self.active;
        for (i, tab) in self.tabs.iter().enumerate().filter(|(i, _)| *i != index) {
            // If the page being hidden has keyboard focus, hand it to the
            // window first so it cannot keep receiving typing.
            if previous == i {
                let _ = tab.raw(cx).focus_parent();
            }
            // Most tabs are already hidden; telling the engine again costs a
            // call into it on every tab switch.
            tab.webview.update(cx, |w, _| {
                if w.visible() {
                    w.hide();
                }
            });
        }
        self.active = index;
        let tab = &mut self.tabs[index];
        tab.webview.update(cx, |w, _| w.show());
        set_memory_level(tab, true, cx);
        let url = address::display(&tab.url);
        let focus_page = !address::is_new_tab(&tab.url);
        if focus_page {
            let _ = tab.raw(cx).focus();
        }
        self.address.update(cx, |input, cx| input.set_value(url, window, cx));
        self.find_count = None;
        self.update_window_title(window);
        self.save_session();
        self.flush_if_due();
        cx.notify();
    }

    fn close_tab(&mut self, index: usize, window: &mut Window, cx: &mut Context<Self>) {
        if index >= self.tabs.len() {
            return;
        }
        let tab = self.tabs.remove(index);
        if !address::is_new_tab(&tab.url) {
            self.closed.push(tab.url.clone());
        }
        // Dropping the entity removes the native page.
        tab.webview.update(cx, |w, _| w.hide());
        drop(tab);

        if self.tabs.is_empty() {
            self.active = 0;
            self.open_tab(address::NEW_TAB_URL, true, window, cx);
            return;
        }
        let next = if index < self.active || self.active >= self.tabs.len() {
            self.active.saturating_sub(1)
        } else {
            self.active.min(self.tabs.len() - 1)
        };
        self.activate(next, window, cx);
    }

    fn cycle_tab(&mut self, step: isize, window: &mut Window, cx: &mut Context<Self>) {
        let n = self.tabs.len() as isize;
        if n > 1 {
            let next = (self.active as isize + step).rem_euclid(n) as usize;
            self.activate(next, window, cx);
        }
    }

    /// Opens a built-in page, reusing a tab that already shows it.
    fn open_internal(&mut self, url: &str, window: &mut Window, cx: &mut Context<Self>) {
        let page = address::internal_page(url);
        if let Some(i) = self.tabs.iter().position(|t| address::internal_page(&t.url) == page) {
            self.activate(i, window, cx);
        } else if address::is_new_tab(&self.active_url()) {
            self.load(url, window, cx);
        } else {
            self.open_tab(url, true, window, cx);
        }
    }

    // ----- navigation -----------------------------------------------------

    fn navigate(&mut self, input: &str, window: &mut Window, cx: &mut Context<Self>) {
        if let Some(url) = address::to_url(input, self.search_template()) {
            self.load(&url, window, cx);
        }
    }

    fn load(&mut self, url: &str, window: &mut Window, cx: &mut Context<Self>) {
        let Some(tab) = self.tabs.get_mut(self.active) else { return };
        tab.url = url.to_string();
        tab.loading = true;
        let raw = tab.webview.read(cx).raw();
        let _ = raw.load_url(url);
        let _ = raw.focus();
        self.address.update(cx, |input, cx| input.set_value(address::display(url), window, cx));
        cx.notify();
    }

    fn focus_address(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.address.update(cx, |input, cx| {
            input.focus(window, cx);
            input.select_all(window, cx);
        });
    }

    fn set_zoom(&mut self, zoom: f64, cx: &mut Context<Self>) {
        let Some(tab) = self.tabs.get_mut(self.active) else { return };
        tab.zoom = zoom;
        let _ = tab.webview.read(cx).set_zoom(zoom);
        cx.notify();
    }

    fn step_zoom(&mut self, up: bool, cx: &mut Context<Self>) {
        let current = self.active_tab().map_or(1.0, |t| t.zoom);
        let next = if up {
            ZOOM_LEVELS.iter().copied().find(|z| *z > current + 0.001)
        } else {
            ZOOM_LEVELS.iter().rev().copied().find(|z| *z < current - 0.001)
        };
        if let Some(z) = next {
            self.set_zoom(z, cx);
        }
    }

    fn set_page_fullscreen(&mut self, on: bool, window: &mut Window) {
        if on == self.page_fullscreen {
            return;
        }
        let was_full = self.page_fullscreen || self.immersive;
        self.page_fullscreen = on;
        // The window is fullscreen exactly while either mode is on, so
        // leaving a video's fullscreen keeps F11's, and vice versa.
        let now_full = self.page_fullscreen || self.immersive;
        if was_full != now_full {
            window.toggle_fullscreen();
        }
    }

    // ----- find in page ---------------------------------------------------

    fn open_find(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.take_focus_from_page(cx);
        self.find_open = true;
        self.find_input.update(cx, |input, cx| {
            input.focus(window, cx);
            input.select_all(window, cx);
        });
        cx.notify();
    }

    fn close_find(&mut self, cx: &mut Context<Self>) {
        if !self.find_open {
            return;
        }
        self.find_open = false;
        self.find_count = None;
        self.raw(cx, |w| {
            let _ = w.evaluate_script("getSelection().removeAllRanges()");
            let _ = w.focus();
        });
        cx.notify();
    }

    /// Uses Chromium's `window.find`, which selects and scrolls to the match.
    fn find(&mut self, backwards: bool, restart: bool, cx: &mut Context<Self>) {
        let query = self.find_input.read(cx).value().to_string();
        let Some(tab) = self.active_tab() else { return };
        let q = serde_json::to_string(&query).unwrap_or_default();
        let script = format!(
            r#"(() => {{
  const q = {q}, s = getSelection();
  if ({restart}) s.removeAllRanges();
  if (!q) return 0;
  window.find(q, false, {backwards}, true, false, false, false);
  const t = (document.body ? document.body.innerText : "").toLowerCase(), ql = q.toLowerCase();
  let n = 0, i = 0;
  while ((i = t.indexOf(ql, i)) !== -1 && n < 9999) {{ n++; i += ql.length; }}
  return n;
}})()"#
        );
        let (events, id) = (self.events.clone(), tab.id);
        let _ = tab.raw(cx).evaluate_script_with_callback(&script, move |result| {
            let n = result.trim().parse().unwrap_or(0);
            let _ = events.unbounded_send((id, PageEvent::FindResult(n)));
        });
    }

    // ----- commands -------------------------------------------------------

    fn run(&mut self, cmd: Cmd, window: &mut Window, cx: &mut Context<Self>) {
        match cmd {
            Cmd::NewTab => self.open_tab(address::NEW_TAB_URL, true, window, cx),
            Cmd::NewWindow => cx.defer(|cx| open_window(Vec::new(), 0, false, cx)),
            Cmd::NewPrivateWindow => cx.defer(|cx| open_window(Vec::new(), 0, true, cx)),
            Cmd::CloseTab => self.close_tab(self.active, window, cx),
            Cmd::ReopenClosedTab => {
                if let Some(url) = self.closed.pop() {
                    self.open_tab(&url, true, window, cx);
                }
            }
            Cmd::NextTab => self.cycle_tab(1, window, cx),
            Cmd::PrevTab => self.cycle_tab(-1, window, cx),
            // Ctrl+1..8 picks that tab, Ctrl+9 the last one, as in other browsers.
            Cmd::SelectTab(n) => {
                let index = if n == 9 { self.tabs.len().saturating_sub(1) } else { n - 1 };
                self.activate(index, window, cx);
            }
            Cmd::FocusAddress => {
                self.take_focus_from_page(cx);
                self.focus_address(window, cx);
            }
            Cmd::ToggleBookmark => {
                let Some(tab) = self.active_tab() else { return };
                if tab.url.is_empty() || address::is_internal(&tab.url) {
                    return;
                }
                let (url, title) = (tab.url.clone(), tab.display_title());
                self.lib.borrow_mut().toggle_bookmark(&url, &title);
                cx.refresh_windows();
            }
            Cmd::Find => {
                if self.find_open && self.find_input.read(cx).focus_handle(cx).is_focused(window) {
                    self.close_find(cx);
                } else {
                    self.open_find(window, cx);
                }
            }
            Cmd::Panel(panel) => {
                if self.sidebar == Some(panel) {
                    self.sidebar = None;
                } else {
                    self.sidebar = Some(panel);
                    self.sidebar_opened += 1;
                    if panel == Panel::History {
                        self.take_focus_from_page(cx);
                        self.history_search.update(cx, |i, cx| i.focus(window, cx));
                    }
                }
                cx.notify();
            }
            Cmd::Settings => {
                self.sidebar = None;
                self.open_internal(address::SETTINGS_URL, window, cx);
            }
            Cmd::ToggleBookmarksBar => {
                {
                    let mut lib = self.lib.borrow_mut();
                    lib.settings.show_bookmarks_bar = !lib.settings.show_bookmarks_bar;
                    lib.save_settings();
                }
                cx.refresh_windows();
            }
            Cmd::CheckForUpdates => crate::update::check_and_install(self.lib.clone(), cx),
            Cmd::ViewSource => {
                let url = self.active_url();
                // Only real sites have source to show; built-in pages and
                // blank tabs say why instead of doing nothing.
                if address::is_internal(&url) {
                    self.show_notice(Icons::Code, "Built-in Chrust pages have no page source.", cx);
                } else if url.starts_with("http") {
                    self.open_tab(&format!("view-source:{url}"), true, window, cx);
                } else {
                    self.show_notice(Icons::Code, "This page has no source to show.", cx);
                }
            }
            Cmd::Fullscreen => {
                let was_full = self.page_fullscreen || self.immersive;
                self.immersive = !self.immersive;
                let now_full = self.page_fullscreen || self.immersive;
                if was_full != now_full {
                    window.toggle_fullscreen();
                }
                cx.notify();
            }
            Cmd::Reload => {
                if let Some(tab) = self.tabs.get_mut(self.active) {
                    tab.loading = true;
                    tab.navigation = None;
                }
                self.raw(cx, |w| {
                    let _ = w.reload();
                });
                cx.notify();
            }
            Cmd::Stop => {
                self.raw(cx, native::stop);
                if let Some(tab) = self.tabs.get_mut(self.active) {
                    tab.loading = false;
                    tab.navigation = None;
                }
                cx.notify();
            }
            Cmd::Back => {
                // A move between history entries only reports a load when
                // there is somewhere to go; otherwise the spinner would never
                // end.
                let moved = self.raw(cx, |w| native::can_go_back(w) && native::go_back(w));
                if moved == Some(true) {
                    if let Some(tab) = self.tabs.get_mut(self.active) {
                        tab.loading = true;
                        tab.navigation = None;
                    }
                    cx.notify();
                }
            }
            Cmd::Forward => {
                let moved = self.raw(cx, |w| native::can_go_forward(w) && native::go_forward(w));
                if moved == Some(true) {
                    if let Some(tab) = self.tabs.get_mut(self.active) {
                        tab.loading = true;
                        tab.navigation = None;
                    }
                    cx.notify();
                }
            }
            Cmd::Home => {
                self.load(address::NEW_TAB_URL, window, cx);
                self.focus_address(window, cx);
            }
            Cmd::DevTools => {
                self.raw(cx, |w| w.open_devtools());
            }
            Cmd::ZoomIn => self.step_zoom(true, cx),
            Cmd::ZoomOut => self.step_zoom(false, cx),
            Cmd::ZoomReset => self.set_zoom(1.0, cx),
            Cmd::Print => {
                self.raw(cx, |w| {
                    let _ = w.evaluate_script("window.print()");
                });
            }
            Cmd::CopyUrl => {
                let url = self.active_url();
                if !url.is_empty() {
                    cx.write_to_clipboard(ClipboardItem::new_string(url));
                    self.show_notice(Icons::Copy, "Link copied to the clipboard.", cx);
                }
            }
            Cmd::ImportChrome => {
                let text = self.import_chrome(cx);
                self.show_notice(Icons::Import, text, cx);
            }
            Cmd::CloseOverlays => {
                if self.find_open {
                    self.close_find(cx);
                } else if self.sidebar.is_some() {
                    self.sidebar = None;
                    cx.notify();
                } else if self.immersive {
                    self.run(Cmd::Fullscreen, window, cx);
                }
            }
        }
    }

    fn import_chrome(&mut self, cx: &mut Context<Self>) -> String {
        let result = import::import_chrome(&self.lib.borrow().bookmarks);
        let text = match result {
            Ok(result) => {
                let added = result.added.len();
                self.lib.borrow_mut().add_bookmarks(result.added);
                let from = result.profiles.join(", ");
                match (added, result.found) {
                    (_, 0) => format!("No bookmarks found in Chrome ({from})."),
                    (0, found) => format!("All {found} Chrome bookmarks are already in Chrust."),
                    (added, _) => format!("Imported {added} bookmarks from Chrome ({from})."),
                }
            }
            Err(err) => err,
        };
        cx.refresh_windows();
        text
    }

    fn save_session(&self) {
        if let Some(id) = self.session_id {
            let tabs = self.tabs.iter().map(|t| t.url.clone()).collect();
            self.lib.borrow_mut().update_window(id, WindowSession { tabs, active: self.active });
        }
    }

    fn update_window_title(&mut self, window: &mut Window) {
        let name = if self.private { "Chrust Private" } else { "Chrust" };
        let title = match self.active_tab() {
            Some(tab) => format!("{} - {name}", tab.display_title()),
            None => name.into(),
        };
        // Pages rewrite their titles while they load; only tell the system
        // when the title it shows would actually change.
        if self.window_title == title {
            return;
        }
        self.window_title.clone_from(&title);
        window.set_window_title(&title);
    }

    // ----- events ---------------------------------------------------------

    fn on_address_event(
        &mut self,
        input: &Entity<InputState>,
        event: &InputEvent,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        match event {
            InputEvent::PressEnter { .. } => {
                let text = input.read(cx).value().to_string();
                self.navigate(&text, window, cx);
            }
            InputEvent::Focus => {
                input.update(cx, |input, cx| input.select_all(window, cx));
                cx.notify();
            }
            InputEvent::Blur => {
                // Put the real address back if the user typed and walked away.
                if let Some(url) = self.active_tab().map(|t| address::display(&t.url)) {
                    input.update(cx, |input, cx| input.set_value(url, window, cx));
                }
                cx.notify();
            }
            InputEvent::Change => {}
        }
    }

    fn on_find_event(&mut self, _: &Entity<InputState>, event: &InputEvent, _: &mut Window, cx: &mut Context<Self>) {
        match event {
            InputEvent::Change => self.find(false, true, cx),
            InputEvent::PressEnter { shift, .. } => self.find(*shift, false, cx),
            _ => {}
        }
    }

    fn on_page_event(&mut self, id: TabId, event: PageEvent, window: &mut Window, cx: &mut Context<Self>) {
        let Some(index) = self.tab_index(id) else { return };
        let is_active = index == self.active;
        // Only the active tab can move the address bar, and building this for
        // every event of every tab is wasted work.
        let old_display = is_active.then(|| address::display(&self.tabs[index].url));
        let mut url_changed = false;
        match event {
            PageEvent::TitleChanged(title) => {
                // Pages rewrite document.title constantly (counters, SPAs,
                // ads); only react when it actually changed.
                if title.is_empty() || self.tabs[index].title == title {
                    return;
                }
                if !self.private {
                    self.lib.borrow_mut().update_title(&self.tabs[index].url, &title);
                }
                self.tabs[index].title = title;
            }
            PageEvent::LoadStarted(url) => {
                let tab = &mut self.tabs[index];
                tab.loading = true;
                tab.navigation = None;
                url_changed = tab.url != url;
                // A new document: the old document's title no longer applies.
                // A reload of the same page keeps it until the new one arrives.
                if url_changed {
                    tab.title.clear();
                }
                tab.url = url;
            }
            PageEvent::LoadFinished(url) => {
                let tab = &mut self.tabs[index];
                tab.loading = false;
                tab.navigation = None;
                url_changed = tab.url != url;
                tab.url = url;
                if !self.private {
                    let (url, title) = (tab.url.clone(), tab.title.clone());
                    self.lib.borrow_mut().record_visit(&url, &title);
                }
            }
            PageEvent::Navigation(_id, url, status) => {
                // The result describes the last navigation only; ignore it
                // when another one has started or finished since.
                let tab = &mut self.tabs[index];
                tab.navigation = Some(status);
                if status == NavigationStatus::Failed && tab.loading {
                    tab.loading = false;
                }
                // Ignore the title the engine synthesised for the failed
                // page in favour of the address the user was going to.
                if status == NavigationStatus::Failed && tab.url != url {
                    let old = std::mem::replace(&mut tab.url, url);
                    if !self.private {
                        self.lib.borrow_mut().update_title(&old, &tab.title);
                    }
                    url_changed = true;
                }
            }
            PageEvent::UrlMaybeChanged => {
                let tab = &mut self.tabs[index];
                if let Ok(url) = tab.webview.read(cx).raw().url() {
                    url_changed = tab.url != url;
                    tab.url = url;
                }
            }
            PageEvent::OpenInNewTab(url) => {
                self.open_tab(&url, true, window, cx);
                return;
            }
            PageEvent::Shortcut(shortcut) => {
                self.run(shortcut.into(), window, cx);
                return;
            }
            PageEvent::DownloadStarted(path) => {
                let name = file_name(&path);
                self.lib.borrow_mut().download_started(path);
                self.notice = Some(Notice { icon: Icons::Download, text: format!("Downloading {name}…"), folder: None });
                cx.refresh_windows();
            }
            PageEvent::DownloadFinished { path, success } => {
                let name = path.as_deref().map(file_name).unwrap_or_else(|| "file".into());
                self.notice = Some(if success {
                    Notice {
                        icon: Icons::CircleCheck,
                        text: format!("Downloaded {name}"),
                        folder: path
                            .as_deref()
                            .and_then(Path::parent)
                            .map(PathBuf::from)
                            .or_else(|| Some(storage::downloads_dir())),
                    }
                } else {
                    Notice { icon: Icons::TriangleAlert, text: format!("Download failed: {name}"), folder: None }
                });
                self.lib.borrow_mut().download_finished(path, success);
                cx.refresh_windows();
            }
            PageEvent::Fullscreen(on) => {
                if is_active {
                    self.set_page_fullscreen(on, window);
                }
            }
            PageEvent::Audio(on) => self.tabs[index].audible = on,
            PageEvent::FindResult(n) => {
                if is_active && self.find_open {
                    self.find_count = Some(n);
                }
            }
            PageEvent::Internal(msg) => {
                self.on_internal(id, &msg, window, cx);
                return;
            }
        }

        if url_changed {
            self.save_session();
        }
        if is_active {
            let editing = self.address.read(cx).focus_handle(cx).is_focused(window);
            // Keep what the user is typing, but follow the page when the
            // address bar still shows the old address (e.g. a search from
            // the new tab page while the bar has focus).
            let untouched = old_display
                .as_deref()
                .is_some_and(|old| self.address.read(cx).value().as_ref() == old);
            if url_changed && (!editing || untouched) {
                let url = address::display(&self.tabs[index].url);
                self.address.update(cx, |input, cx| input.set_value(url, window, cx));
            }
            self.update_window_title(window);
        }
        self.flush_if_due();
        cx.notify();
    }

    /// Commands from the settings and new tab pages. `tab.rs` only forwards
    /// these when they come from an `aurora` page.
    fn on_internal(&mut self, tab: TabId, msg: &Value, window: &mut Window, cx: &mut Context<Self>) {
        match msg["cmd"].as_str().unwrap_or_default() {
            "set" => {
                let key = msg["key"].as_str().unwrap_or_default();
                let changed = {
                    let mut lib = self.lib.borrow_mut();
                    let changed = lib.settings.set(key, &msg["value"]);
                    if changed {
                        lib.save_settings();
                    }
                    changed
                };
                if !changed {
                    return;
                }
                match key {
                    "theme" => library::apply_theme(cx),
                    "save_passwords" => {
                        let on = self.lib.borrow().settings.save_passwords && !self.private;
                        for t in &self.tabs {
                            native::set_save_passwords(t.raw(cx), on);
                        }
                    }
                    _ => {}
                }
                if matches!(key, "theme" | "background" | "background_url" | "show_clock" | "search_engine" | "site_icons" | "smooth_animations") {
                    // Show the change on open new tab pages straight away.
                    for t in self.tabs.iter_mut().filter(|t| address::is_new_tab(&t.url)) {
                        // A settings change is not a load progress; do not
                        // flash the spinner while the page rebuilds.
                        t.loading = false;
                        // Visited navigation entries keep the old title in
                        // history; this one is a rebuild of the same page.
                        if !self.private {
                            let (url, title) = (t.url.clone(), t.title.clone());
                            self.lib.borrow_mut().update_title(&url, &title);
                        }
                        let _ = t.raw(cx).reload();
                    }
                }
                cx.refresh_windows();
            }
            "background_image" => {
                let data = msg["data"].as_str().unwrap_or_default();
                let bytes = base64::engine::general_purpose::STANDARD.decode(data).unwrap_or_default();
                if bytes.is_empty() || bytes.len() > 20 * 1024 * 1024 {
                    self.notify_page(tab, "That image could not be used.", cx);
                    return;
                }
                let path = storage::background_file();
                let saved = path.parent().is_some_and(|d| std::fs::create_dir_all(d).is_ok())
                    && std::fs::write(&path, &bytes).is_ok();
                if saved {
                    {
                        let mut lib = self.lib.borrow_mut();
                        lib.settings.background = "custom".into();
                        lib.settings.background_url.clear();
                        lib.save_settings();
                    }
                    self.notify_page(tab, "Background updated. Open a new tab to see it.", cx);
                } else {
                    self.notify_page(tab, "Could not save the image.", cx);
                }
            }
            "clear_history" => {
                self.lib.borrow_mut().clear_history();
                self.notify_page(tab, "History cleared.", cx);
                cx.refresh_windows();
            }
            "clear_data" => {
                let ok = self.raw(cx, |w| w.clear_all_browsing_data().is_ok()).unwrap_or(false);
                self.notify_page(tab, if ok { "Cookies, cache and site data cleared." } else { "Could not clear browsing data." }, cx);
            }
            "import_chrome" => {
                let text = self.import_chrome(cx);
                self.notify_page(tab, &text, cx);
            }
            "open_devtools" => self.run(Cmd::DevTools, window, cx),
            "open_downloads" => self.run(Cmd::Panel(Panel::Downloads), window, cx),
            "open_webstore" => self.open_tab("https://chromewebstore.google.com/", true, window, cx),
            "install_extension" => {
                let input = msg["input"].as_str().unwrap_or_default();
                self.install_extension(input, tab, cx);
            }
            "remove_extension" => {
                if let Some(id) = msg["id"].as_str() {
                    crate::extensions::uninstall(id);
                    cx.refresh_windows();
                }
            }
            "open_releases" => {
                let url = format!("https://github.com/{}/releases/latest", crate::update::REPO);
                self.open_tab(&url, true, window, cx);
            }
            "check_for_updates" => {
                self.run(Cmd::CheckForUpdates, window, cx);
                self.notify_page(tab, "Checking GitHub for updates…", cx);
            }
            "remove_bookmark" => {
                if let Some(url) = msg["url"].as_str() {
                    self.lib.borrow_mut().remove_bookmark(url);
                    self.mirror_bookmarks(tab, cx);
                    cx.refresh_windows();
                }
            }
            "edit_bookmark" => {
                let (Some(url), Some(title), Some(new_url)) =
                    (msg["url"].as_str(), msg["title"].as_str(), msg["new_url"].as_str())
                else {
                    return;
                };
                let new_url = new_url.trim();
                let new_url = if new_url.contains("://") {
                    new_url.to_string()
                } else {
                    address::to_url(new_url, self.search_template()).unwrap_or_default()
                };
                // Only web addresses, so a bookmark can never run a script.
                if !(new_url.starts_with("https://") || new_url.starts_with("http://")) {
                    self.notify_page(tab, "Bookmarks must be http or https addresses.", cx);
                    return;
                }
                let title = title.trim();
                let title = if title.is_empty() { new_url.as_str() } else { title };
                if self.lib.borrow_mut().edit_bookmark(url, title, &new_url) {
                    self.mirror_bookmarks(tab, cx);
                    cx.refresh_windows();
                }
            }
            _ => {}
        }
    }

    // ----- rendering ------------------------------------------------------

    fn site_icon(&self, url: &str, cx: &mut Context<Self>) -> AnyElement {
        let muted = cx.theme().muted_foreground;
        match address::internal_page(url) {
            // Tab and bookmark icons match a classic Chrome favicon (16 px
            // at 100%, 20 px here to read well next to the tab text).
            Some("settings") => {
                return Icon::new(Icons::Settings).size(px(20.)).text_color(muted).into_any_element();
            }
            Some(_) => return logo().size(px(20.)).flex_none().rounded_sm().into_any_element(),
            None => {}
        }
        match favicons::icon(url, cx) {
            Some(path) => img(path).size(px(20.)).flex_none().rounded_sm().into_any_element(),
            None => Icon::new(Icons::Globe).size(px(20.)).text_color(muted).into_any_element(),
        }
    }

    fn render_tab_strip(&self, cx: &mut Context<Self>) -> impl IntoElement + use<> {
        let animate = self.animate();
        let tabs: Vec<_> = self
            .tabs
            .iter()
            .map(|t| (t.id, t.display_title(), t.loading, t.audible, t.muted, self.site_icon(&t.url, cx)))
            .collect();
        let theme = cx.theme();
        let (active_bg, hover_bg, fg, muted) = (theme.background, theme.secondary, theme.foreground, theme.muted_foreground);

        h_flex()
            .id("tabs")
            .h_full()
            .flex_1()
            .min_w_0()
            .items_end()
            .gap_0p5()
            .pl_1()
            .overflow_x_hidden()
            .when(self.private, |el| {
                el.child(
                    h_flex()
                        .id("private-badge")
                        .occlude()
                        .mb_1()
                        .mr_1()
                        .px_2()
                        .h(px(24.))
                        .gap_1()
                        .items_center()
                        .rounded_full()
                        .text_xs()
                        .text_color(rgb(0xffffff))
                        .bg(rgb(PRIVATE_COLOR))
                        .child(Icon::new(Icons::VenetianMask).size_3p5())
                        .child("Private"),
                )
            })
            .children(tabs.into_iter().enumerate().map(|(i, (id, title, loading, audible, muted_tab, icon))| {
                let selected = i == self.active;
                let group: SharedString = format!("tab-{id}").into();
                let tab = h_flex()
                    .id(("tab", id))
                    .group(group.clone())
                    // Take the mouse from the title bar's drag area underneath,
                    // which on Windows would turn every click into a window move.
                    .occlude()
                    .relative()
                    .h(px(32.))
                    .min_w(px(56.))
                    .max_w(px(236.))
                    .flex_1()
                    .pl_2p5()
                    .pr_1()
                    .gap_2()
                    .items_center()
                    .rounded_t_lg()
                    .text_xs()
                    .cursor_pointer()
                    .when(selected, |el| el.bg(active_bg).text_color(fg))
                    .when(!selected, |el| el.text_color(muted).hover(|s| s.bg(hover_bg).text_color(fg)))
                    .on_click(cx.listener(move |this, _, window, cx| {
                        if let Some(index) = this.tab_index(id) {
                            this.activate(index, window, cx);
                        }
                    }))
                    .on_mouse_down(
                        MouseButton::Middle,
                        cx.listener(move |this, _, window, cx| {
                            if let Some(index) = this.tab_index(id) {
                                this.close_tab(index, window, cx);
                            }
                        }),
                    )
                    .child(div().flex_none().child(if loading {
                        Icon::new(Icons::LoaderCircle)
                            .size_4()
                            .text_color(cx.theme().primary)
                            .with_animation(
                                ("spin", id),
                                Animation::new(Duration::from_millis(800)).repeat(),
                                |icon, t| icon.transform(Transformation::rotate(percentage(t))),
                            )
                            .into_any_element()
                    } else {
                        icon
                    }))
                    .child(div().flex_1().min_w_0().truncate().child(title))
                    .when(audible || muted_tab, |el| {
                        el.child(
                            Button::new(("mute", id))
                                .ghost()
                                .xsmall()
                                .icon(if muted_tab { Icons::VolumeX } else { Icons::Volume2 })
                                .tooltip(if muted_tab { "Unmute tab" } else { "Mute tab" })
                                .on_click(cx.listener(move |this, _, _, cx| {
                                    cx.stop_propagation();
                                    if let Some(tab) = this.tab_index(id).and_then(|i| this.tabs.get_mut(i)) {
                                        tab.muted = !tab.muted;
                                        native::set_muted(tab.webview.read(cx).raw(), tab.muted);
                                    }
                                    cx.notify();
                                })),
                        )
                    })
                    .child(
                        div().when(!selected, |d| d.invisible().group_hover(group, |s| s.visible())).child(
                            Button::new(("close-tab", id))
                                .ghost()
                                .xsmall()
                                .icon(Icons::X)
                                .on_click(cx.listener(move |this, _, window, cx| {
                                    cx.stop_propagation();
                                    if let Some(index) = this.tab_index(id) {
                                        this.close_tab(index, window, cx);
                                    }
                                })),
                        ),
                    );
                if animate {
                    tab.with_animation(
                        ("tab-in", id),
                        Animation::new(Duration::from_millis(220)).with_easing(ease_out_quint()),
                        |el, d| el.opacity(d).mt(px(6. * (1. - d))),
                    )
                    .into_any_element()
                } else {
                    tab.into_any_element()
                }
            }))
            .child(
                div().id("new-tab-slot").occlude().pb_1().pl_1().child(
                    Button::new("new-tab")
                        .ghost()
                        .small()
                        .icon(Icons::Plus)
                        .tooltip("New tab (Ctrl+T)")
                        .on_click(cx.listener(|this, _, window, cx| this.run(Cmd::NewTab, window, cx))),
                ),
            )
    }

    fn render_toolbar(&self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement + use<> {
        let (can_back, can_forward) = self
            .raw(cx, |w| (native::can_go_back(w), native::can_go_forward(w)))
            .unwrap_or((false, false));
        let tab = self.active_tab();
        let loading = tab.is_some_and(is_navigating);
        let zoom = tab.map_or(1.0, |t| t.zoom);
        let url = self.active_url();
        let lib = self.lib.borrow();
        let bookmarked = lib.is_bookmarked(&url);
        let downloading = lib.active_downloads() > 0;
        let dev_button = lib.settings.show_dev_tools_button;
        drop(lib);
        let can_bookmark = !url.is_empty() && !address::is_internal(&url);
        let editing = self.address.read(cx).focus_handle(cx).is_focused(window);
        let animate = self.animate();
        let theme = cx.theme().clone();

        let site_icon: AnyElement = if editing || url.is_empty() || address::is_new_tab(&url) {
            Icon::new(Icons::Search).size_4().text_color(theme.muted_foreground).into_any_element()
        } else if address::is_internal(&url) {
            logo().size_4().flex_none().rounded_sm().into_any_element()
        } else if address::is_secure(&url) {
            Icon::new(Icons::Lock).size_4().text_color(theme.muted_foreground).into_any_element()
        } else {
            Icon::new(Icons::TriangleAlert).size_4().text_color(theme.warning).into_any_element()
        };

        let panel_button = |id: &'static str, icon: Icons, tip: &'static str, panel: Panel, cx: &mut Context<Self>| {
            Button::new(id)
                .ghost()
                .icon(icon)
                .tooltip(tip)
                .selected(self.sidebar == Some(panel))
                .on_click(cx.listener(move |this, _, w, cx| this.run(Cmd::Panel(panel), w, cx)))
        };

        h_flex()
            .relative()
            .h(px(46.))
            .px_2()
            .gap_0p5()
            .items_center()
            .bg(theme.background)
            .child(
                Button::new("back")
                    .ghost()
                    .icon(Icons::ArrowLeft)
                    .tooltip("Back (Alt+Left)")
                    .disabled(!can_back)
                    .on_click(cx.listener(|this, _, w, cx| this.run(Cmd::Back, w, cx))),
            )
            .child(
                Button::new("forward")
                    .ghost()
                    .icon(Icons::ArrowRight)
                    .tooltip("Forward (Alt+Right)")
                    .disabled(!can_forward)
                    .on_click(cx.listener(|this, _, w, cx| this.run(Cmd::Forward, w, cx))),
            )
            .child(
                h_flex()
                    .gap_0p5()
                    .items_center()
                    .child(if loading {
                        Button::new("stop")
                            .ghost()
                            .icon(Icons::X)
                            .tooltip("Stop loading")
                            .on_click(cx.listener(|this, _, w, cx| this.run(Cmd::Stop, w, cx)))
                    } else {
                        Button::new("reload")
                            .ghost()
                            .icon(Icons::RotateCw)
                            .tooltip("Reload (F5)")
                            .on_click(cx.listener(|this, _, w, cx| this.run(Cmd::Reload, w, cx)))
                    })
                    .when(loading, |el| {
                        // The current document already renders while the
                        // engine fetches the next one: show that work is
                        // happening next to Stop.
                        el.child(
                            Icon::new(Icons::LoaderCircle).size_3p5().text_color(theme.muted_foreground).with_animation(
                                "toolbar-spin",
                                Animation::new(Duration::from_millis(800)).repeat(),
                                |icon, t| icon.transform(Transformation::rotate(percentage(t))),
                            ),
                        )
                    }),
            )
            .child(
                Button::new("home")
                    .ghost()
                    .icon(Icons::House)
                    .tooltip("New tab page (Alt+Home)")
                    .on_click(cx.listener(|this, _, w, cx| this.run(Cmd::Home, w, cx))),
            )
            // The address bar: a pill with the site icon, the input, a go
            // button and the bookmark star.
            .child(
                h_flex()
                    .flex_1()
                    .min_w_0()
                    .mx_1p5()
                    .h(px(34.))
                    .pl_3()
                    .pr_1()
                    .gap_2()
                    .items_center()
                    .rounded_full()
                    .bg(theme.secondary)
                    .border_1()
                    .border_color(if editing { theme.ring } else { theme.secondary })
                    .when(editing, |el| el.shadow_sm())
                    .child(site_icon)
                    .child(div().flex_1().min_w_0().child(Input::new(&self.address).appearance(false)))
                    .when(zoom != 1.0, |el| {
                        el.child(
                            Button::new("zoom-badge")
                                .ghost()
                                .xsmall()
                                .label(format!("{:.0}%", zoom * 100.))
                                .tooltip("Reset zoom (Ctrl+0)")
                                .on_click(cx.listener(|this, _, w, cx| this.run(Cmd::ZoomReset, w, cx))),
                        )
                    })
                    .child(
                        Button::new("bookmark")
                            .ghost()
                            .xsmall()
                            .icon(if bookmarked { Icons::StarFill } else { Icons::Star })
                            .tooltip(if bookmarked { "Remove bookmark (Ctrl+D)" } else { "Bookmark this page (Ctrl+D)" })
                            .disabled(!can_bookmark)
                            .on_click(cx.listener(|this, _, w, cx| this.run(Cmd::ToggleBookmark, w, cx))),
                    )
                    .child(
                        div()
                            .id("go")
                            .flex_none()
                            .size(px(26.))
                            .rounded_full()
                            .flex()
                            .items_center()
                            .justify_center()
                            .cursor_pointer()
                            .bg(theme.primary)
                            .text_color(theme.primary_foreground)
                            .hover(|s| s.bg(theme.primary_hover))
                            .active(|s| s.opacity(0.8))
                            .on_click(cx.listener(|this, _, window, cx| {
                                let text = this.address.read(cx).value().to_string();
                                if text.trim().is_empty() {
                                    this.focus_address(window, cx);
                                } else {
                                    this.navigate(&text, window, cx);
                                }
                            }))
                            .child(Icon::new(Icons::ArrowRight).size_3p5()),
                    ),
            )
            .child(panel_button("bookmarks", Icons::Bookmark, "Bookmarks (Ctrl+Shift+O)", Panel::Bookmarks, cx))
            .child(panel_button("history", Icons::Clock, "History (Ctrl+H)", Panel::History, cx))
            .child(
                div()
                    .relative()
                    .child(panel_button("downloads", Icons::Download, "Downloads (Ctrl+J)", Panel::Downloads, cx))
                    .when(downloading, |el| {
                        el.child(
                            div()
                                .absolute()
                                .top(px(5.))
                                .right(px(5.))
                                .size(px(7.))
                                .rounded_full()
                                .bg(theme.primary),
                        )
                    }),
            )
            .when(dev_button, |el| {
                el.child(
                    Button::new("devtools")
                        .ghost()
                        .icon(Icons::SquareTerminal)
                        .tooltip("Developer tools (F12)")
                        .on_click(cx.listener(|this, _, w, cx| this.run(Cmd::DevTools, w, cx))),
                )
            })
            .child(panel_button("menu", Icons::EllipsisVertical, "Menu (Alt+F)", Panel::Menu, cx))
            // Thin loading bar along the bottom edge.
            .when(loading, |el| {
                let bar = div()
                    .absolute()
                    .top_0()
                    .h_full()
                    .w(relative(0.3))
                    .rounded_full()
                    .bg(theme.primary);
                el.child(
                    div().absolute().bottom_0().left_0().w_full().h(px(2.)).overflow_hidden().child(if animate {
                        bar.with_animation(
                            "progress",
                            Animation::new(Duration::from_millis(1100)).repeat().with_easing(ease_in_out),
                            |el, d| el.left(relative(d * 1.3 - 0.3)),
                        )
                        .into_any_element()
                    } else {
                        bar.left_0().w_full().opacity(0.6).into_any_element()
                    }),
                )
            })
    }

    fn render_bookmarks_bar(&self, cx: &mut Context<Self>) -> impl IntoElement + use<> {
        let bookmarks = self.lib.borrow().bookmarks.clone();
        let icons: Vec<_> = bookmarks.iter().map(|b| self.site_icon(&b.url, cx)).collect();
        let theme = cx.theme();
        h_flex()
            .id("bookmarks-bar")
            .h(px(32.))
            .px_2()
            .gap_0p5()
            .items_center()
            .overflow_x_hidden()
            .bg(theme.background)
            .border_b_1()
            .border_color(theme.border)
            .when(bookmarks.is_empty(), |el| {
                el.child(
                    div()
                        .px_2()
                        .text_xs()
                        .text_color(theme.muted_foreground)
                        .child("Bookmark pages with Ctrl+D or the star, or import them from Chrome in the menu."),
                )
            })
            .children(bookmarks.into_iter().zip(icons).enumerate().map(|(i, (b, icon))| {
                let url = b.url.clone();
                h_flex()
                    .id(("bm", i))
                    .flex_none()
                    .h(px(26.))
                    .px_2()
                    .gap_1p5()
                    .items_center()
                    .rounded_md()
                    .text_xs()
                    .cursor_pointer()
                    .hover(|s| s.bg(theme.secondary))
                    .on_click(cx.listener(move |this, _, window, cx| this.load(&url, window, cx)))
                    .child(icon)
                    .child(truncate(&b.title, 22))
            }))
    }

    fn render_find_bar(&self, cx: &mut Context<Self>) -> impl IntoElement + use<> {
        let theme = cx.theme();
        let count = match self.find_count {
            Some(0) if !self.find_input.read(cx).value().is_empty() => "No matches".to_string(),
            Some(1) => "1 match".to_string(),
            Some(n) if n > 0 => format!("{n} matches"),
            _ => String::new(),
        };
        h_flex()
            .h(px(42.))
            .px_3()
            .gap_2()
            .items_center()
            .bg(theme.background)
            .border_b_1()
            .border_color(theme.border)
            .child(Icon::new(Icons::Search).size_4().text_color(theme.muted_foreground))
            .child(div().w(px(300.)).child(Input::new(&self.find_input).small()))
            .child(div().min_w(px(80.)).text_xs().text_color(theme.muted_foreground).child(count))
            .child(
                Button::new("find-prev")
                    .ghost()
                    .small()
                    .icon(Icons::ChevronUp)
                    .tooltip("Previous (Shift+Enter)")
                    .on_click(cx.listener(|this, _, _, cx| this.find(true, false, cx))),
            )
            .child(
                Button::new("find-next")
                    .ghost()
                    .small()
                    .icon(Icons::ChevronDown)
                    .tooltip("Next (Enter)")
                    .on_click(cx.listener(|this, _, _, cx| this.find(false, false, cx))),
            )
            .child(div().flex_1())
            .child(
                Button::new("find-close")
                    .ghost()
                    .small()
                    .icon(Icons::X)
                    .on_click(cx.listener(|this, _, _, cx| this.close_find(cx))),
            )
    }

    fn render_notice(&self, cx: &mut Context<Self>) -> Option<impl IntoElement + use<>> {
        let notice = self.notice.as_ref()?;
        let folder = notice.folder.clone();
        let theme = cx.theme();
        Some(
            h_flex()
                .h(px(36.))
                .px_3()
                .gap_2()
                .items_center()
                .text_sm()
                .bg(theme.secondary)
                .border_b_1()
                .border_color(theme.border)
                .child(Icon::new(notice.icon).size_4().text_color(theme.primary))
                .child(div().flex_1().min_w_0().truncate().child(notice.text.clone()))
                .when_some(folder, |el, folder| {
                    el.child(
                        Button::new("open-folder")
                            .ghost()
                            .xsmall()
                            .icon(Icons::FolderOpen)
                            .label("Show in folder")
                            .on_click(move |_, _, _| open_path(&folder)),
                    )
                })
                .child(
                    Button::new("dismiss-notice")
                        .ghost()
                        .xsmall()
                        .icon(Icons::X)
                        .on_click(cx.listener(|this, _, _, cx| {
                            this.notice = None;
                            cx.notify();
                        })),
                ),
        )
    }

    fn render_sidebar(&self, panel: Panel, cx: &mut Context<Self>) -> AnyElement {
        let (title, body): (&str, AnyElement) = match panel {
            Panel::Menu => ("Menu", self.render_menu(cx).into_any_element()),
            Panel::Bookmarks => ("Bookmarks", self.render_bookmarks_panel(cx).into_any_element()),
            Panel::History => ("History", self.render_history_panel(cx).into_any_element()),
            Panel::Downloads => ("Downloads", self.render_downloads_panel(cx).into_any_element()),
        };
        let theme = cx.theme();
        let content = v_flex()
            .w(px(SIDEBAR_W))
            .h_full()
            .flex_none()
            .child(
                h_flex()
                    .h(px(48.))
                    .px_4()
                    .items_center()
                    .justify_between()
                    .child(div().text_base().font_weight(FontWeight::SEMIBOLD).child(title))
                    .child(
                        Button::new("close-sidebar")
                            .ghost()
                            .small()
                            .icon(Icons::X)
                            .on_click(cx.listener(|this, _, _, cx| {
                                this.sidebar = None;
                                cx.notify();
                            })),
                    ),
            )
            .child(div().flex_1().min_h_0().child(body));

        let shell = div()
            .h_full()
            .flex_none()
            .overflow_hidden()
            .bg(theme.sidebar)
            .border_l_1()
            .border_color(theme.border)
            .w(px(SIDEBAR_W))
            .child(content);
        if self.animate() {
            shell
                .with_animation(
                    ("sidebar", self.sidebar_opened),
                    Animation::new(Duration::from_millis(240)).with_easing(ease_out_quint()),
                    |el, d| el.w(px(SIDEBAR_W * d)).opacity(0.4 + 0.6 * d),
                )
                .into_any_element()
        } else {
            shell.into_any_element()
        }
    }

    fn menu_row(
        &self,
        id: &'static str,
        icon: Icons,
        label: &'static str,
        keys: &'static str,
        cmd: Cmd,
        cx: &mut Context<Self>,
    ) -> impl IntoElement + use<> {
        let theme = cx.theme();
        h_flex()
            .id(id)
            .h(px(36.))
            .px_3()
            .gap_3()
            .items_center()
            .rounded_lg()
            .text_sm()
            .cursor_pointer()
            .hover(|s| s.bg(theme.secondary_hover))
            .active(|s| s.bg(theme.secondary_active))
            .on_click(cx.listener(move |this, _, window, cx| {
                // Actions that show something in the page close the menu.
                if !matches!(cmd, Cmd::ZoomIn | Cmd::ZoomOut | Cmd::ZoomReset) {
                    this.sidebar = None;
                }
                this.run(cmd, window, cx);
            }))
            .child(Icon::new(icon).size_4().text_color(theme.muted_foreground))
            .child(div().flex_1().child(label))
            .child(div().text_xs().text_color(theme.muted_foreground).child(keys))
    }

    fn render_menu(&self, cx: &mut Context<Self>) -> impl IntoElement + use<> {
        let zoom = self.active_tab().map_or(1.0, |t| t.zoom);
        let theme = cx.theme().clone();
        let sep = || div().h(px(1.)).mx_3().my_1p5().bg(theme.border);
        v_flex()
            .id("menu")
            .size_full()
            .px_2()
            .pb_3()
            .overflow_y_scroll()
            .child(self.menu_row("m-newtab", Icons::Plus, "New tab", "Ctrl+T", Cmd::NewTab, cx))
            .child(self.menu_row("m-newwin", Icons::AppWindow, "New window", "Ctrl+N", Cmd::NewWindow, cx))
            .child(self.menu_row("m-private", Icons::VenetianMask, "New private window", "Ctrl+Shift+N", Cmd::NewPrivateWindow, cx))
            .child(sep())
            .child(
                h_flex()
                    .h(px(40.))
                    .px_3()
                    .gap_3()
                    .items_center()
                    .text_sm()
                    .child(Icon::new(Icons::ZoomIn).size_4().text_color(theme.muted_foreground))
                    .child(div().flex_1().child("Zoom"))
                    .child(
                        Button::new("m-zoom-out")
                            .ghost()
                            .small()
                            .icon(Icons::Minus)
                            .on_click(cx.listener(|this, _, w, cx| this.run(Cmd::ZoomOut, w, cx))),
                    )
                    .child(
                        Button::new("m-zoom-reset")
                            .ghost()
                            .small()
                            .label(format!("{:.0}%", zoom * 100.))
                            .on_click(cx.listener(|this, _, w, cx| this.run(Cmd::ZoomReset, w, cx))),
                    )
                    .child(
                        Button::new("m-zoom-in")
                            .ghost()
                            .small()
                            .icon(Icons::Plus)
                            .on_click(cx.listener(|this, _, w, cx| this.run(Cmd::ZoomIn, w, cx))),
                    ),
            )
            .child(self.menu_row("m-find", Icons::Search, "Find in page", "Ctrl+F", Cmd::Find, cx))
            .child(self.menu_row("m-print", Icons::Printer, "Print", "Ctrl+P", Cmd::Print, cx))
            .child(self.menu_row("m-full", Icons::Fullscreen, "Full screen", "F11", Cmd::Fullscreen, cx))
            .child(sep())
            .child(self.menu_row("m-bookmarks", Icons::Bookmark, "Bookmarks", "Ctrl+Shift+O", Cmd::Panel(Panel::Bookmarks), cx))
            .child(self.menu_row("m-bar", Icons::PanelTop, "Show or hide bookmarks bar", "Ctrl+Shift+B", Cmd::ToggleBookmarksBar, cx))
            .child(self.menu_row("m-history", Icons::Clock, "History", "Ctrl+H", Cmd::Panel(Panel::History), cx))
            .child(self.menu_row("m-downloads", Icons::Download, "Downloads", "Ctrl+J", Cmd::Panel(Panel::Downloads), cx))
            .child(self.menu_row("m-import", Icons::Import, "Import bookmarks from Chrome", "", Cmd::ImportChrome, cx))
            .child(sep())
            .child(self.menu_row("m-devtools", Icons::SquareTerminal, "Developer tools", "F12", Cmd::DevTools, cx))
            .child(self.menu_row("m-source", Icons::Code, "View page source", "Ctrl+U", Cmd::ViewSource, cx))
            .child(self.menu_row("m-copy", Icons::Copy, "Copy page link", "", Cmd::CopyUrl, cx))
            .child(sep())
            .child(self.menu_row("m-settings", Icons::Settings, "Settings", "Ctrl+,", Cmd::Settings, cx))
            .when(crate::update::CAN_INSTALL, |menu| {
                menu.child(self.menu_row("m-update", Icons::Download, "Check for updates", "", Cmd::CheckForUpdates, cx))
            })
    }

    fn list_row(&self, id: ElementId, icon: AnyElement, title: String, detail: String, cx: &App) -> Stateful<Div> {
        let theme = cx.theme();
        h_flex()
            .id(id)
            .group("row")
            .h(px(46.))
            .px_2()
            .gap_3()
            .items_center()
            .rounded_lg()
            .cursor_pointer()
            .hover(|s| s.bg(theme.secondary_hover))
            .child(div().flex_none().size(px(28.)).rounded_md().bg(theme.secondary).flex().items_center().justify_center().child(icon))
            .child(
                v_flex()
                    .flex_1()
                    .min_w_0()
                    .child(div().text_sm().truncate().child(title))
                    .child(div().text_xs().truncate().text_color(theme.muted_foreground).child(detail)),
            )
    }

    fn render_bookmarks_panel(&self, cx: &mut Context<Self>) -> impl IntoElement + use<> {
        let bookmarks = self.lib.borrow().bookmarks.clone();
        let icons: Vec<_> = bookmarks.iter().map(|b| self.site_icon(&b.url, cx)).collect();
        let rows: Vec<_> = bookmarks
            .into_iter()
            .zip(icons)
            .enumerate()
            .map(|(i, (b, icon))| {
                let (url, remove_url) = (b.url.clone(), b.url.clone());
                self.list_row(("bmp", i).into(), icon, b.title.clone(), host_or_url(&b.url), cx)
                    .on_click(cx.listener(move |this, _, window, cx| this.load(&url, window, cx)))
                    .child(
                        div().invisible().group_hover("row", |s| s.visible()).child(
                            Button::new(("bmp-del", i))
                                .ghost()
                                .xsmall()
                                .icon(Icons::Trash)
                                .tooltip("Remove bookmark")
                                .on_click(cx.listener(move |this, _, _, cx| {
                                    cx.stop_propagation();
                                    this.lib.borrow_mut().remove_bookmark(&remove_url);
                                    cx.refresh_windows();
                                })),
                        ),
                    )
            })
            .collect();
        let empty = rows.is_empty();
        v_flex()
            .size_full()
            .child(
                h_flex()
                    .px_3()
                    .pb_2()
                    .gap_2()
                    .child(
                        Button::new("bm-add")
                            .small()
                            .icon(Icons::Star)
                            .label("Bookmark this page")
                            .on_click(cx.listener(|this, _, w, cx| this.run(Cmd::ToggleBookmark, w, cx))),
                    )
                    .child(
                        Button::new("bm-import")
                            .small()
                            .ghost()
                            .icon(Icons::Import)
                            .label("Import from Chrome")
                            .on_click(cx.listener(|this, _, w, cx| this.run(Cmd::ImportChrome, w, cx))),
                    ),
            )
            .child(
                v_flex()
                    .id("bm-list")
                    .flex_1()
                    .min_h_0()
                    .px_2()
                    .pb_3()
                    .overflow_y_scroll()
                    .when(empty, |el| el.child(empty_state(Icons::Bookmark, "No bookmarks yet.", cx)))
                    .children(rows),
            )
    }

    fn render_history_panel(&self, cx: &mut Context<Self>) -> impl IntoElement + use<> {
        let query = self.history_search.read(cx).value().to_lowercase();
        let entries: Vec<_> = self
            .lib
            .borrow()
            .history
            .iter()
            .rev()
            .filter(|e| query.is_empty() || contains_folded(&e.title, &query) || contains_folded(&e.url, &query))
            .take(300)
            .cloned()
            .collect();
        let now = storage::now();
        let icons: Vec<_> = entries.iter().map(|e| self.site_icon(&e.url, cx)).collect();
        let rows: Vec<_> = entries
            .into_iter()
            .zip(icons)
            .enumerate()
            .map(|(i, (e, icon))| {
                let url = e.url.clone();
                let (del_url, visited) = (e.url.clone(), e.visited);
                let title = if e.title.is_empty() { e.url.clone() } else { e.title.clone() };
                let detail = format!("{} · {}", host_or_url(&e.url), ago(now, e.visited));
                self.list_row(("hist", i).into(), icon, title, detail, cx)
                    .on_click(cx.listener(move |this, _, window, cx| this.load(&url, window, cx)))
                    .child(
                        div().invisible().group_hover("row", |s| s.visible()).child(
                            Button::new(("hist-del", i))
                                .ghost()
                                .xsmall()
                                .icon(Icons::X)
                                .tooltip("Remove from history")
                                .on_click(cx.listener(move |this, _, _, cx| {
                                    cx.stop_propagation();
                                    this.lib.borrow_mut().remove_history(&del_url, visited);
                                    cx.notify();
                                })),
                        ),
                    )
            })
            .collect();
        let empty = rows.is_empty();
        let private_note = self.private;
        v_flex()
            .size_full()
            .child(div().px_3().pb_2().child(Input::new(&self.history_search).small().cleanable(true)))
            .when(private_note, |el| {
                el.child(
                    div()
                        .px_4()
                        .pb_2()
                        .text_xs()
                        .text_color(cx.theme().muted_foreground)
                        .child("Pages you visit in this private window are not added."),
                )
            })
            .child(
                v_flex()
                    .id("hist-list")
                    .flex_1()
                    .min_h_0()
                    .px_2()
                    .overflow_y_scroll()
                    .when(empty, |el| el.child(empty_state(Icons::Clock, "Nothing here yet.", cx)))
                    .children(rows),
            )
            .child(
                h_flex().p_3().child(
                    Button::new("hist-clear")
                        .small()
                        .ghost()
                        .icon(Icons::Trash)
                        .label("Clear all history")
                        .on_click(cx.listener(|this, _, _, cx| {
                            this.lib.borrow_mut().clear_history();
                            cx.notify();
                        })),
                ),
            )
    }

    fn render_downloads_panel(&self, cx: &mut Context<Self>) -> impl IntoElement + use<> {
        let downloads = self.lib.borrow().downloads.clone();
        let now = storage::now();
        let rows: Vec<_> = downloads
            .into_iter()
            .rev()
            .enumerate()
            .map(|(i, d)| {
                let theme = cx.theme();
                let (icon, status) = match d.state {
                    DownloadState::InProgress => (
                        Icon::new(Icons::LoaderCircle)
                            .size_4()
                            .text_color(theme.primary)
                            .with_animation(("dl-spin", i), Animation::new(Duration::from_millis(800)).repeat(), |icon, t| {
                                icon.transform(Transformation::rotate(percentage(t)))
                            })
                            .into_any_element(),
                        "Downloading…".to_string(),
                    ),
                    DownloadState::Done => (
                        Icon::new(Icons::File).size_4().text_color(theme.muted_foreground).into_any_element(),
                        format!("Done · {}", ago(now, d.started)),
                    ),
                    DownloadState::Failed => (
                        Icon::new(Icons::TriangleAlert).size_4().text_color(theme.danger).into_any_element(),
                        "Failed or cancelled".to_string(),
                    ),
                };
                let (open, folder) = (d.path.clone(), d.path.parent().map(PathBuf::from).unwrap_or_else(storage::downloads_dir));
                let done = d.state == DownloadState::Done;
                self.list_row(("dl", i).into(), icon, file_name(&d.path), status, cx)
                    .when(done, |el| el.on_click(move |_, _, _| open_path(&open)))
                    .child(
                        Button::new(("dl-folder", i))
                            .ghost()
                            .xsmall()
                            .icon(Icons::FolderOpen)
                            .tooltip("Show in folder")
                            .on_click(move |_, _, cx| {
                                cx.stop_propagation();
                                open_path(&folder);
                            }),
                    )
            })
            .collect();
        let empty = rows.is_empty();
        v_flex()
            .size_full()
            .child(
                v_flex()
                    .id("dl-list")
                    .flex_1()
                    .min_h_0()
                    .px_2()
                    .overflow_y_scroll()
                    .when(empty, |el| el.child(empty_state(Icons::Download, "No downloads yet.", cx)))
                    .children(rows),
            )
            .child(
                h_flex()
                    .p_3()
                    .gap_2()
                    .child(
                        Button::new("dl-open-folder")
                            .small()
                            .icon(Icons::FolderOpen)
                            .label("Open Downloads")
                            .on_click(|_, _, _| open_path(&storage::downloads_dir())),
                    )
                    .child(
                        Button::new("dl-clear")
                            .small()
                            .ghost()
                            .label("Clear list")
                            .on_click(cx.listener(|this, _, _, cx| {
                                this.lib.borrow_mut().clear_downloads();
                                cx.notify();
                            })),
                    ),
            )
    }
}

impl Focusable for Browser {
    fn focus_handle(&self, _: &App) -> FocusHandle {
        self.focus_handle.clone()
    }
}

macro_rules! bind_actions {
    ($el:expr, $cx:expr, { $($action:ident => $cmd:expr),* $(,)? }) => {
        $el$(.on_action($cx.listener(|this, _: &$action, window, cx| this.run($cmd, window, cx))))*
    };
}

impl Render for Browser {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        perf::mark_once("first frame");
        self.flush_if_due();
        if self.pending.is_some() {
            self.restore(window, cx);
        }
        let page = self.active_tab().map(|t| t.webview.clone());
        let root = v_flex()
            .size_full()
            .track_focus(&self.focus_handle)
            .bg(cx.theme().title_bar)
            .text_color(cx.theme().foreground);
        let root = bind_actions!(root, cx, {
            NewTab => Cmd::NewTab,
            NewWindow => Cmd::NewWindow,
            NewPrivateWindow => Cmd::NewPrivateWindow,
            CloseTab => Cmd::CloseTab,
            ReopenClosedTab => Cmd::ReopenClosedTab,
            NextTab => Cmd::NextTab,
            PrevTab => Cmd::PrevTab,
            FocusAddress => Cmd::FocusAddress,
            Reload => Cmd::Reload,
            GoBack => Cmd::Back,
            GoForward => Cmd::Forward,
            GoHome => Cmd::Home,
            ToggleBookmark => Cmd::ToggleBookmark,
            OpenDevTools => Cmd::DevTools,
            Find => Cmd::Find,
            ShowHistory => Cmd::Panel(Panel::History),
            ShowDownloads => Cmd::Panel(Panel::Downloads),
            ShowBookmarks => Cmd::Panel(Panel::Bookmarks),
            ShowMenu => Cmd::Panel(Panel::Menu),
            OpenSettings => Cmd::Settings,
            ToggleBookmarksBar => Cmd::ToggleBookmarksBar,
            ViewSource => Cmd::ViewSource,
            CheckForUpdates => Cmd::CheckForUpdates,
            ToggleFullscreen => Cmd::Fullscreen,
            ZoomIn => Cmd::ZoomIn,
            ZoomOut => Cmd::ZoomOut,
            ZoomReset => Cmd::ZoomReset,
            Print => Cmd::Print,
            CloseOverlays => Cmd::CloseOverlays,
        });

        // Video fullscreen and F11 show only the page.
        if self.page_fullscreen || self.immersive {
            return root.bg(gpui_kit::black()).child(div().flex_1().min_h_0().w_full().children(page));
        }

        let show_bar = self.lib.borrow().settings.show_bookmarks_bar;
        let tab_strip = self.render_tab_strip(cx);
        let toolbar = self.render_toolbar(window, cx);
        let bookmarks_bar = show_bar.then(|| self.render_bookmarks_bar(cx));
        let find_bar = self.find_open.then(|| self.render_find_bar(cx));
        let notice = self.render_notice(cx);
        let sidebar = self.sidebar.map(|p| self.render_sidebar(p, cx));

        root.child(TitleBar::new().child(tab_strip))
            .child(toolbar)
            .children(bookmarks_bar)
            .children(find_bar)
            .children(notice)
            .child(
                h_flex()
                    .flex_1()
                    .min_h_0()
                    .w_full()
                    .bg(cx.theme().background)
                    .child(div().flex_1().min_w_0().h_full().children(page))
                    .children(sidebar),
            )
    }
}

/// The logo drawn in tabs and on the built-in pages. Replace
/// `assets/chrust/chrust-64.png` to rebrand; Cargo does not track that file,
/// so run `cargo clean -p chrust` afterwards.
pub const LOGO_PNG: &[u8] = include_bytes!("../assets/chrust/chrust-64.png");
/// Address of the logo on built-in pages. Changes with every build so the
/// webview can cache it instead of refetching it for each page and frame.
pub const LOGO_PATH: &str = concat!("/logo-", env!("CARGO_PKG_VERSION"), ".png");

/// The Chrust logo, for built-in pages in tabs and the address bar.
fn logo() -> Img {
    static IMAGE: std::sync::LazyLock<std::sync::Arc<Image>> =
        std::sync::LazyLock::new(|| std::sync::Arc::new(Image::from_bytes(ImageFormat::Png, LOGO_PNG.to_vec())));
    img(IMAGE.clone())
}

fn empty_state(icon: Icons, text: &'static str, cx: &App) -> impl IntoElement {
    let muted = cx.theme().muted_foreground;
    v_flex()
        .py_12()
        .gap_3()
        .items_center()
        .text_sm()
        .text_color(muted)
        .child(Icon::new(icon).size_8().text_color(muted.opacity(0.6)))
        .child(text)
}

fn truncate(s: &str, max: usize) -> String {
    // The common case is a short title, and its byte length alone settles it.
    if s.len() <= max {
        return s.to_string();
    }
    if s.chars().count() <= max {
        return s.to_string();
    }
    let mut out: String = s.chars().take(max.saturating_sub(1)).collect();
    out.push('…');
    out
}

/// Whether `haystack` contains `needle`, ignoring ASCII case, without making a
/// lowercase copy of the haystack: the history list asks this of thousands of
/// entries on every frame while a search is typed.
fn contains_folded(haystack: &str, needle_lower: &str) -> bool {
    if needle_lower.is_empty() {
        return true;
    }
    if haystack.is_ascii() {
        let (haystack, needle) = (haystack.as_bytes(), needle_lower.as_bytes());
        return haystack
            .windows(needle.len())
            .any(|window| window.eq_ignore_ascii_case(needle));
    }
    haystack.to_lowercase().contains(needle_lower)
}

fn host_or_url(url: &str) -> String {
    url::Url::parse(url)
        .ok()
        .and_then(|u| u.host_str().map(|h| h.trim_start_matches("www.").to_string()))
        .unwrap_or_else(|| url.to_string())
}

/// "just now", "5 min ago", "3 h ago", "2 days ago".
fn ago(now: u64, then: u64) -> String {
    let s = now.saturating_sub(then);
    match s {
        0..60 => "just now".into(),
        60..3600 => format!("{} min ago", s / 60),
        3600..86400 => format!("{} h ago", s / 3600),
        86400..172800 => "yesterday".into(),
        _ => format!("{} days ago", s / 86400),
    }
}

/// True while the tab has a load in flight and the engine has not reported
/// it as failed. A page whose navigation fails partway (DNS error, dropped
/// connection, stop) becomes false once WebView2 reports the failure.
fn is_navigating(tab: &Tab) -> bool {
    tab.loading && tab.navigation != Some(NavigationStatus::Failed)
}

/// Tells the page engine whether a tab is in use, so hidden pages can give
/// memory back and shown ones can recompose quickly. The engine is only told
/// when that actually changes, which is what a tab switch mostly does.
fn set_memory_level(tab: &mut Tab, active: bool, cx: &App) {
    if tab.memory_active == active {
        return;
    }
    tab.memory_active = active;
    #[cfg(windows)]
    {
        use wry::WebViewExtWindows as _;
        let level = if active { wry::MemoryUsageLevel::Normal } else { wry::MemoryUsageLevel::Low };
        let _ = tab.raw(cx).set_memory_usage_level(level);
    }
    #[cfg(not(windows))]
    let _ = (tab, active, cx);
}

fn file_name(path: &Path) -> String {
    path.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_else(|| "file".into())
}

/// Opens a folder in Explorer, or a file with its default app.
fn open_path(path: &Path) {
    #[cfg(windows)]
    let _ = std::process::Command::new("explorer").arg(path).spawn();
    #[cfg(not(windows))]
    let _ = std::process::Command::new("xdg-open").arg(path).spawn();
}

#[cfg(test)]
mod tests {
    use super::{ZOOM_LEVELS, ago, contains_folded, truncate};

    #[test]
    fn relative_times() {
        assert_eq!(ago(1000, 990), "just now");
        assert_eq!(ago(10_000, 10_000 - 300), "5 min ago");
        assert_eq!(ago(100_000, 100_000 - 7200), "2 h ago");
        assert_eq!(ago(1_000_000, 1_000_000 - 90_000), "yesterday");
        assert_eq!(ago(1_000_000, 1_000_000 - 3 * 86400), "3 days ago");
    }

    #[test]
    fn folded_search() {
        assert!(contains_folded("Hello World", "hello"));
        assert!(contains_folded("Hello World", "o w"));
        assert!(!contains_folded("Hello World", "z"));
        assert!(contains_folded("anything", ""));
        assert!(contains_folded("Ärger im Büro", "ärger"));
        assert!(!contains_folded("abc", "abcd"));
    }

    #[test]
    fn shortens_long_titles() {
        assert_eq!(truncate("short", 10), "short");
        assert_eq!(truncate("abcdefghij", 5), "abcd…");
        assert_eq!(truncate("abcdef", 6), "abcdef");
    }

    #[test]
    fn zoom_levels_sorted() {
        assert!(ZOOM_LEVELS.windows(2).all(|w| w[0] < w[1]));
        assert!(ZOOM_LEVELS.contains(&1.0));
    }
}
