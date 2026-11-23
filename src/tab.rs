//! One browser tab: a WebView2 instance plus the state the UI shows for it.

use std::borrow::Cow;
use std::path::PathBuf;

use futures::channel::mpsc::UnboundedSender;
use gpui_kit::{App, AppContext as _, Entity, Window};
use gpui_webview::WebView;
use wry::{NewWindowResponse, PageLoadEvent, WebContext, WebViewBuilder, http};

use crate::library::Shared;
use crate::native::{self, NativeEvent};
use crate::{address, browser, extensions, pages, storage};

pub type TabId = u64;

/// What a built-in page is rendered with: the shared browser state and
/// whether the tab is private. Pages ask for it on every request, so they
/// show current data without being reloaded by hand.
pub struct PageContext {
    pub lib: Shared,
    pub private: bool,
}

pub fn page_context(lib: &Shared, private: bool) -> PageContext {
    PageContext { lib: lib.clone(), private }
}

/// Something that happened inside a page. WebView2 reports these from its own
/// callbacks; they reach the UI through a channel so no UI state is borrowed
/// while WebView2 is calling us.
#[derive(Debug)]
pub enum PageEvent {
    TitleChanged(String),
    LoadStarted(String),
    LoadFinished(String),
    /// Navigation result from engines that report it (WebView2): the tab,
    /// the address that was being opened, and whether it rendered. Unlike
    /// `UrlMaybeChanged` this cannot be produced by page scripts.
    Navigation(TabId, String, crate::native::NavigationStatus),
    /// The page changed its URL without a load (history.pushState, #hash).
    UrlMaybeChanged,
    OpenInNewTab(String),
    Shortcut(Shortcut),
    DownloadStarted(PathBuf),
    DownloadFinished { path: Option<PathBuf>, success: bool },
    Fullscreen(bool),
    Audio(bool),
    FindResult(usize),
    /// A command from a built-in page (settings, new tab).
    Internal(serde_json::Value),
}

/// Browser shortcuts pressed while the page has keyboard focus. GPUI never
/// sees those key presses, so a page script forwards them.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Shortcut {
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
    History,
    Downloads,
    Bookmarks,
    Settings,
    ToggleBookmarksBar,
    ViewSource,
    Fullscreen,
    CheckForUpdates,
}

impl Shortcut {
    fn parse(action: &str) -> Option<Self> {
        Some(match action {
            "new_tab" => Self::NewTab,
            "new_window" => Self::NewWindow,
            "new_private_window" => Self::NewPrivateWindow,
            "close_tab" => Self::CloseTab,
            "reopen_tab" => Self::ReopenClosedTab,
            "next_tab" => Self::NextTab,
            "prev_tab" => Self::PrevTab,
            "focus_address" => Self::FocusAddress,
            "bookmark" => Self::ToggleBookmark,
            "find" => Self::Find,
            "history" => Self::History,
            "downloads" => Self::Downloads,
            "bookmarks" => Self::Bookmarks,
            "settings" => Self::Settings,
            "bookmarks_bar" => Self::ToggleBookmarksBar,
            "view_source" => Self::ViewSource,
            "check_for_updates" => Self::CheckForUpdates,
            "fullscreen" => Self::Fullscreen,
            _ => {
                let n: usize = action.strip_prefix("tab_")?.parse().ok()?;
                if !(1..=9).contains(&n) {
                    return None;
                }
                Self::SelectTab(n)
            }
        })
    }
}

/// Injected into every top-level page. Only reports *that* something
/// happened; the browser reads the real URL from WebView2 itself, so a page
/// cannot spoof the address bar through this channel.
const PAGE_SCRIPT: &str = r#"
(() => {
  if (window.__aurora) return;
  window.__aurora = true;
  const send = (m) => { try { window.ipc.postMessage(JSON.stringify(m)); } catch (_) {} };
  const urlChanged = () => send({ kind: "url" });
  for (const name of ["pushState", "replaceState"]) {
    const original = history[name];
    history[name] = function (...args) { const r = original.apply(this, args); urlChanged(); return r; };
  }
  addEventListener("popstate", urlChanged);
  addEventListener("hashchange", urlChanged);
  addEventListener("keydown", (e) => {
    const k = e.key.length === 1 ? e.key.toLowerCase() : e.key;
    const c = e.ctrlKey && !e.altKey && !e.metaKey, s = e.shiftKey;
    let a = null;
    if (c) {
      if (k === "t") a = s ? "reopen_tab" : "new_tab";
      else if (k === "n") a = s ? "new_private_window" : "new_window";
      else if (k === "w" || k === "F4") a = "close_tab";
      else if (k === "l") a = "focus_address";
      else if (k === "d" && !s) a = "bookmark";
      else if (k === "f" && !s) a = "find";
      else if (k === "h" && !s) a = "history";
      else if (k === "j" && !s) a = "downloads";
      else if (k === "o" && s) a = "bookmarks";
      else if (k === "b" && s) a = "bookmarks_bar";
      else if (k === "," ) a = "settings";
      else if (k === "u" && !s) a = "view_source";
      else if (k === "Tab") a = s ? "prev_tab" : "next_tab";
      else if (k === "PageDown") a = "next_tab";
      else if (k === "PageUp") a = "prev_tab";
      else if (/^[1-9]$/.test(k) && !s) a = "tab_" + k;
    } else if (e.altKey && !e.ctrlKey && k === "d") a = "focus_address";
    else if (k === "F6") a = "focus_address";
    else if (k === "F11") a = "fullscreen";
    if (a) { e.preventDefault(); e.stopImmediatePropagation(); send({ kind: "shortcut", action: a }); }
  }, true);
})();
"#;

pub struct Tab {
    pub id: TabId,
    pub webview: Entity<WebView>,
    pub title: String,
    pub url: String,
    pub loading: bool,
    /// Latest engine navigation result; only valid while `url` matches the
    /// navigation it describes.
    pub navigation: Option<crate::native::NavigationStatus>,
    pub audible: bool,
    pub muted: bool,
    pub zoom: f64,
}

/// Chrome Web Store pages send their install button at the browser itself,
/// which is what makes "Add to Chrome" fail in a shell that is not Chrome.
/// Chrust's answer is a page that asks it to fetch and install the extension
/// and then shows whether that worked.
fn install_redirect(context: &PageContext, target: Option<&str>) -> http::Response<Cow<'static, [u8]>> {
    let id = target.and_then(crate::extensions::extension_id);
    let (status, body) = match id {
        Some(id) => (200, pages::install(&context.lib.borrow(), id)),
        None => (400, pages::install_nothing()),
    };
    http::Response::builder()
        .status(status)
        .header("Content-Type", "text/html; charset=utf-8")
        .header("Cache-Control", "no-store")
        .body(Cow::Owned(body.into_bytes()))
        .expect("static response parts are valid")
}

/// Built-in page name from a custom-protocol request URI, which arrives as
/// `http://aurora.settings/...` or `aurora://settings/...`.
fn page_of(uri: &http::Uri) -> String {
    let host = uri.host().unwrap_or_default();
    host.strip_prefix("aurora.").unwrap_or(host).to_string()
}

fn serve(context: &PageContext, request: &http::Request<Vec<u8>>) -> http::Response<Cow<'static, [u8]>> {
    let lib = &context.lib;
    let private = context.private;
    let page = page_of(request.uri());
    let path = request.uri().path();
    // The store page's install button posts here; the form's target names
    // the extension ("chromewebstore" is the store itself, not one).
    if page == "install" && path == "/add" {
        let target = request.uri().query().and_then(|q| q.split('&').find_map(|p| p.strip_prefix("target=")));
        return install_redirect(context, target);
    }
    let (status, mime, body): (u16, &str, Vec<u8>) = match (page.as_str(), path) {
        // The name carries a fingerprint, so pages can cache the image.
        (_, logo_path) if logo_path == browser::LOGO_PATH => {
            (200, "image/png", browser::LOGO_PNG.to_vec())
        }
        ("newtab", "/background") => match std::fs::read(storage::background_file()) {
            Ok(bytes) => (200, image_mime(&bytes), bytes),
            Err(_) => (404, "text/plain", b"no background".to_vec()),
        },
        // Site icons the browser already cached, so the new tab page does
        // not ask Google again for every tile.
        ("newtab", "/favicon") => {
            let host = request.uri().query().and_then(|q| q.split('&').find_map(|p| p.strip_prefix("h=")));
            match host {
                Some(host) if crate::favicons::safe_host(host) => {
                    let path = storage::favicon_dir().join(format!("{host}.png"));
                    match std::fs::read(&path) {
                        Ok(bytes) if !bytes.is_empty() => (200, "image/png", bytes),
                        _ => (404, "text/plain", b"no icon".to_vec()),
                    }
                }
                _ => (400, "text/plain", b"bad host".to_vec()),
            }
        }
        // Everything the browser reports on the pages: the last update or
        // extension result, shown once.
        ("settings", "/report") | ("newtab", "/report") => {
            let text = lib.borrow_mut().update_message.take().unwrap_or_default();
            (200, "application/json", serde_json::to_vec(&text).unwrap_or_default())
        }
        ("newtab", _) => (200, "text/html; charset=utf-8", pages::new_tab(&lib.borrow(), private).into_bytes()),
        ("settings", _) => (200, "text/html; charset=utf-8", pages::settings(&lib.borrow()).into_bytes()),
        _ => (404, "text/plain", b"Not found".to_vec()),
    };
    // The logo only changes with a build, so it can be cached for good (its
    // address carries the version). Everything else reflects live data.
    let cache = if path == browser::LOGO_PATH { "public, max-age=31536000, immutable" } else { "no-store" };
    http::Response::builder()
        .status(status)
        .header("Content-Type", mime)
        .header("Cache-Control", cache)
        .body(Cow::Owned(body))
        .expect("static response parts are valid")
}

fn image_mime(bytes: &[u8]) -> &'static str {
    match bytes {
        [0x89, b'P', b'N', b'G', ..] => "image/png",
        [0xff, 0xd8, ..] => "image/jpeg",
        [b'G', b'I', b'F', ..] => "image/gif",
        [b'R', b'I', b'F', b'F', _, _, _, _, b'W', b'E', b'B', b'P', ..] => "image/webp",
        _ => "application/octet-stream",
    }
}

impl Tab {
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        id: TabId,
        url: &str,
        private: bool,
        // The tab is the one being shown; hidden tabs must not take keyboard
        // focus away from the window.
        focused: bool,
        context: &mut WebContext,
        lib: Shared,
        events: UnboundedSender<(TabId, PageEvent)>,
        window: &mut Window,
        cx: &mut App,
    ) -> anyhow::Result<Self> {
        let send = move |event: PageEvent| {
            let _ = events.unbounded_send((id, event));
        };
        let save_passwords = !private && lib.borrow().settings.save_passwords;
        let background = crate::themes::css_bg(lib.borrow().palette);
        // Hidden pages get less digital-rights-management CPU; the visible
        // one gets the normal budget.
        let throttling = if focused {
            wry::BackgroundThrottlingPolicy::Suspend
        } else {
            wry::BackgroundThrottlingPolicy::Throttle
        };

        let builder = WebViewBuilder::new_with_web_context(context)
            .with_url(url)
            .with_incognito(private)
            .with_background_color(background)
            .with_devtools(true)
            .with_hotkeys_zoom(true)
            .with_background_throttling(throttling)
            .with_clipboard(true)
            .with_focused(focused)
            .with_back_forward_navigation_gestures(true)
            .with_initialization_script_for_main_only(PAGE_SCRIPT, true)
            .with_custom_protocol("aurora".into(), {
                let page = page_context(&lib, private);
                move |_, request| serve(&page, &request)
            })
            .with_document_title_changed_handler({
                let send = send.clone();
                move |title| send(PageEvent::TitleChanged(title))
            })
            .with_on_page_load_handler({
                let send = send.clone();
                move |event, url| {
                    send(match event {
                        PageLoadEvent::Started => PageEvent::LoadStarted(url),
                        PageLoadEvent::Finished => PageEvent::LoadFinished(url),
                    })
                }
            })
            .with_ipc_handler({
                let send = send.clone();
                move |request| {
                    let Ok(msg) = serde_json::from_str::<serde_json::Value>(request.body()) else {
                        return;
                    };
                    // Built-in page commands are only accepted from built-in pages.
                    let from_internal = request.uri().scheme_str() == Some("http")
                        && request.uri().host().is_some_and(|h| {
                            h.strip_prefix("aurora.").is_some_and(|p| address::INTERNAL_PAGES.contains(&p))
                        });
                    match msg["kind"].as_str() {
                        Some("url") => send(PageEvent::UrlMaybeChanged),
                        Some("shortcut") => {
                            if let Some(s) = msg["action"].as_str().and_then(Shortcut::parse) {
                                send(PageEvent::Shortcut(s));
                            }
                        }
                        Some("aurora") if from_internal => send(PageEvent::Internal(msg)),
                        _ => {}
                    }
                }
            })
            .with_new_window_req_handler({
                let send = send.clone();
                move |url, features| {
                    // Popups that ask for a size (sign-in windows, payment
                    // dialogs) need `window.opener`, so let WebView2 open them
                    // as real popups. Plain target=_blank links become tabs.
                    if features.size.is_some() {
                        NewWindowResponse::Allow
                    } else {
                        send(PageEvent::OpenInNewTab(url));
                        NewWindowResponse::Deny
                    }
                }
            })
            .with_download_started_handler({
                let send = send.clone();
                move |_url, path| {
                    send(PageEvent::DownloadStarted(path.clone()));
                    true
                }
            })
            .with_download_completed_handler({
                let send = send.clone();
                move |_url, path, success| send(PageEvent::DownloadFinished { path, success })
            });
        // Extensions are a Windows feature: WebView2 loads every folder in
        // this path as each view starts. The browser arguments repeat wry's
        // defaults, plus the extension feature for runtimes that need it
        // written out.
        #[cfg(windows)]
        let builder = {
            use wry::WebViewBuilderExtWindows as _;
            let folder = extensions::extensions_dir();
            // The proxy goes in as Chromium's own switch, which is read when
            // the engine's process starts.
            let mut args = String::from(
                "--enable-features=msExtensions,msEdgeExtensions --disable-features=msWebOOUI,msPdfOOUI,msSmartScreenProtection",
            );
            if let Some(proxy) = crate::proxy::configured() {
                args.push(' ');
                args.push_str(&proxy.browser_flag());
            }
            let mut builder = builder.with_browser_extensions_enabled(true).with_additional_browser_args(args);
            if std::fs::create_dir_all(&folder).is_ok() {
                builder = builder.with_extensions_path(folder);
            }
            builder
        };
        // Elsewhere the engine takes the proxy directly.
        #[cfg(not(windows))]
        let builder = match crate::proxy::configured() {
            Some(proxy) => builder.with_proxy_config(proxy.for_webview()),
            None => builder,
        };
        #[cfg(windows)]
        let builder = builder;

        let webview = WebView::build(builder, window, cx)?;
        native::set_save_passwords(webview.raw(), save_passwords);
        native::watch(webview.raw(), move |event| {
            send(match event {
                NativeEvent::Fullscreen(on) => PageEvent::Fullscreen(on),
                NativeEvent::Audio(on) => PageEvent::Audio(on),
            })
        });

        Ok(Self {
            id,
            webview: cx.new(|_| webview),
            title: String::new(),
            url: url.to_string(),
            loading: true,
            navigation: None,
            audible: false,
            muted: false,
            zoom: 1.0,
        })
    }

    pub fn display_title(&self) -> String {
        if !self.title.trim().is_empty() && !self.title.starts_with("http://aurora.") {
            self.title.clone()
        } else {
            match address::internal_page(&self.url) {
                Some("newtab") => "New Tab".into(),
                Some("settings") => "Settings".into(),
                _ => self.url.clone(),
            }
        }
    }

    pub fn raw<'a>(&self, cx: &'a App) -> &'a wry::WebView {
        self.webview.read(cx).raw()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_shortcuts() {
        assert_eq!(Shortcut::parse("new_tab"), Some(Shortcut::NewTab));
        assert_eq!(Shortcut::parse("find"), Some(Shortcut::Find));
        assert_eq!(Shortcut::parse("tab_3"), Some(Shortcut::SelectTab(3)));
        assert_eq!(Shortcut::parse("tab_0"), None);
        assert_eq!(Shortcut::parse("navigate"), None);
    }

    #[test]
    fn request_pages() {
        let uri: http::Uri = "http://aurora.settings/x".parse().unwrap();
        assert_eq!(page_of(&uri), "settings");
        let uri: http::Uri = "aurora://newtab/background".parse().unwrap();
        assert_eq!(page_of(&uri), "newtab");
        assert_eq!(image_mime(&[0x89, b'P', b'N', b'G', 0]), "image/png");
    }
}
