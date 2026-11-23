//! One browser tab: a WebView2 instance plus the state the UI shows for it.

use std::borrow::Cow;
use std::cell::RefCell;
use std::path::PathBuf;
use std::rc::Rc;

use futures::channel::mpsc::UnboundedSender;
use gpui_kit::{App, AppContext as _, Entity, Window};
use gpui_webview::WebView;
use wry::{NewWindowResponse, PageLoadEvent, WebContext, WebViewBuilder, http};

use crate::pages;
use crate::storage::Bookmark;

pub type TabId = u64;

/// Something that happened inside a page. WebView2 reports these from its own
/// callbacks; they reach the UI through a channel so no UI state is borrowed
/// while WebView2 is calling us.
#[derive(Debug)]
pub enum PageEvent {
    TitleChanged(String),
    LoadStarted(String),
    LoadFinished(String),
    /// The page changed its URL without a load (history.pushState, #hash).
    UrlMaybeChanged,
    OpenInNewTab(String),
    Shortcut(Shortcut),
    DownloadStarted(PathBuf),
    DownloadFinished { path: Option<PathBuf>, success: bool },
}

/// Browser shortcuts pressed while the page has keyboard focus. GPUI never
/// sees those key presses, so a page script forwards them.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Shortcut {
    NewTab,
    CloseTab,
    ReopenClosedTab,
    NextTab,
    PrevTab,
    SelectTab(usize),
    FocusAddress,
    ToggleBookmark,
}

impl Shortcut {
    fn parse(action: &str) -> Option<Self> {
        Some(match action {
            "new_tab" => Self::NewTab,
            "close_tab" => Self::CloseTab,
            "reopen_tab" => Self::ReopenClosedTab,
            "next_tab" => Self::NextTab,
            "prev_tab" => Self::PrevTab,
            "focus_address" => Self::FocusAddress,
            "bookmark" => Self::ToggleBookmark,
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
    let action = null;
    if (e.ctrlKey && !e.altKey && !e.metaKey) {
      if (k === "t") action = e.shiftKey ? "reopen_tab" : "new_tab";
      else if (k === "w" || k === "F4") action = "close_tab";
      else if (k === "l") action = "focus_address";
      else if (k === "d") action = "bookmark";
      else if (k === "Tab") action = e.shiftKey ? "prev_tab" : "next_tab";
      else if (k === "PageDown") action = "next_tab";
      else if (k === "PageUp") action = "prev_tab";
      else if (/^[1-9]$/.test(k) && !e.shiftKey) action = "tab_" + k;
    } else if (e.altKey && !e.ctrlKey && k === "d") action = "focus_address";
    else if (k === "F6") action = "focus_address";
    if (action) { e.preventDefault(); e.stopImmediatePropagation(); send({ kind: "shortcut", action }); }
  }, true);
})();
"#;

pub struct Tab {
    pub id: TabId,
    pub webview: Entity<WebView>,
    pub title: String,
    pub url: String,
    pub loading: bool,
}

impl Tab {
    pub fn new(
        id: TabId,
        url: &str,
        context: &mut WebContext,
        bookmarks: Rc<RefCell<Vec<Bookmark>>>,
        events: UnboundedSender<(TabId, PageEvent)>,
        window: &mut Window,
        cx: &mut App,
    ) -> anyhow::Result<Self> {
        let send = move |event: PageEvent| {
            let _ = events.unbounded_send((id, event));
        };

        let builder = WebViewBuilder::new_with_web_context(context)
            .with_url(url)
            .with_devtools(true)
            .with_hotkeys_zoom(true)
            .with_clipboard(true)
            .with_focused(true)
            .with_back_forward_navigation_gestures(true)
            .with_initialization_script_for_main_only(PAGE_SCRIPT, true)
            .with_custom_protocol("aurora".into(), move |_, request| {
                let host = request.uri().host().unwrap_or_default();
                let (status, body) = if host == "aurora.newtab" || host == "newtab" {
                    (200, pages::new_tab(&bookmarks.borrow()))
                } else {
                    (404, "Not found".to_string())
                };
                http::Response::builder()
                    .status(status)
                    .header("Content-Type", "text/html; charset=utf-8")
                    .body(Cow::Owned(body.into_bytes()))
                    .expect("static response parts are valid")
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
                    match msg["kind"].as_str() {
                        Some("url") => send(PageEvent::UrlMaybeChanged),
                        Some("shortcut") => {
                            if let Some(s) = msg["action"].as_str().and_then(Shortcut::parse) {
                                send(PageEvent::Shortcut(s));
                            }
                        }
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
            .with_download_completed_handler(move |_url, path, success| {
                send(PageEvent::DownloadFinished { path, success })
            });

        let webview = WebView::build(builder, window, cx)?;
        Ok(Self {
            id,
            webview: cx.new(|_| webview),
            title: String::new(),
            url: url.to_string(),
            loading: true,
        })
    }

    pub fn display_title(&self) -> String {
        if !self.title.trim().is_empty() {
            self.title.clone()
        } else if crate::address::is_new_tab(&self.url) {
            "New Tab".into()
        } else {
            self.url.clone()
        }
    }
}

#[cfg(test)]
mod tests {
    use super::Shortcut;

    #[test]
    fn parses_shortcuts() {
        assert_eq!(Shortcut::parse("new_tab"), Some(Shortcut::NewTab));
        assert_eq!(Shortcut::parse("tab_3"), Some(Shortcut::SelectTab(3)));
        assert_eq!(Shortcut::parse("tab_0"), None);
        assert_eq!(Shortcut::parse("tab_x"), None);
        assert_eq!(Shortcut::parse("navigate"), None);
    }
}
