//! The browser window: tab strip, toolbar, bookmarks bar and the active page.

use std::cell::RefCell;
use std::path::PathBuf;
use std::rc::Rc;

use futures::StreamExt as _;
use futures::channel::mpsc::{UnboundedSender, unbounded};
use gpui_kit::assets::IconName as Icons;
use gpui_kit::component::{
    ActiveTheme as _, Disableable as _, Icon, IconName, Sizable as _, TitleBar,
    button::{Button, ButtonVariants as _},
    h_flex,
    input::{Input, InputEvent, InputState},
    v_flex,
};
use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::*;
use wry::WebContext;

use crate::address;
use crate::native;
use crate::storage::{self, Bookmark, Session};
use crate::tab::{PageEvent, Shortcut, Tab, TabId};

actions!(
    aurora,
    [
        NewTab,
        CloseTab,
        ReopenClosedTab,
        NextTab,
        PrevTab,
        FocusAddress,
        Reload,
        GoBack,
        GoForward,
        GoHome,
        ToggleBookmark,
        OpenDevTools,
    ]
);

pub fn bind_keys(cx: &mut App) {
    cx.bind_keys([
        KeyBinding::new("ctrl-t", NewTab, None),
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
    ]);
}

/// A one-line notice under the toolbar. GPUI popups would be hidden behind
/// the native page on Windows, so notices live in the browser chrome.
struct Notice {
    text: String,
    folder: Option<PathBuf>,
}

pub struct Browser {
    focus_handle: FocusHandle,
    tabs: Vec<Tab>,
    active: usize,
    next_id: TabId,
    closed: Vec<String>,
    address: Entity<InputState>,
    web_context: WebContext,
    bookmarks: Rc<RefCell<Vec<Bookmark>>>,
    events: UnboundedSender<(TabId, PageEvent)>,
    notice: Option<Notice>,
    _subscriptions: Vec<Subscription>,
}

impl Browser {
    pub fn new(start_urls: Vec<String>, window: &mut Window, cx: &mut Context<Self>) -> Self {
        let address = cx.new(|cx| {
            InputState::new(window, cx).placeholder("Search Google or type a URL")
        });
        let subscriptions = vec![cx.subscribe_in(&address, window, Self::on_address_event)];

        let (events, mut rx) = unbounded::<(TabId, PageEvent)>();
        cx.spawn_in(window, async move |this, cx| {
            while let Some((id, event)) = rx.next().await {
                if this
                    .update_in(cx, |this, window, cx| this.on_page_event(id, event, window, cx))
                    .is_err()
                {
                    break;
                }
            }
        })
        .detach();

        let mut this = Self {
            focus_handle: cx.focus_handle(),
            tabs: Vec::new(),
            active: 0,
            next_id: 1,
            closed: Vec::new(),
            address,
            web_context: WebContext::new(Some(storage::profile_dir())),
            bookmarks: Rc::new(RefCell::new(storage::load_bookmarks())),
            events,
            notice: None,
            _subscriptions: subscriptions,
        };

        // Restore the last session unless URLs were given on the command line.
        let (urls, active) = if !start_urls.is_empty() {
            (start_urls, 0)
        } else if let Some(session) = storage::load_session() {
            let active = session.active.min(session.tabs.len() - 1);
            (session.tabs, active)
        } else {
            (vec![address::NEW_TAB_URL.to_string()], 0)
        };
        for (i, url) in urls.iter().enumerate() {
            this.open_tab(url, i == active, window, cx);
        }
        this
    }

    fn active_tab(&self) -> Option<&Tab> {
        self.tabs.get(self.active)
    }

    fn tab_index(&self, id: TabId) -> Option<usize> {
        self.tabs.iter().position(|t| t.id == id)
    }

    fn raw<R>(&self, cx: &App, f: impl FnOnce(&wry::WebView) -> R) -> Option<R> {
        self.active_tab().map(|tab| f(tab.webview.read(cx).raw()))
    }

    // ----- tabs -----------------------------------------------------------

    fn open_tab(&mut self, url: &str, activate: bool, window: &mut Window, cx: &mut Context<Self>) {
        let id = self.next_id;
        self.next_id += 1;
        let tab = Tab::new(
            id,
            url,
            &mut self.web_context,
            self.bookmarks.clone(),
            self.events.clone(),
            window,
            cx,
        );
        let tab = match tab {
            Ok(tab) => tab,
            Err(err) => {
                log::error!("could not create a WebView2 page: {err:#}");
                self.notice = Some(Notice {
                    text: format!(
                        "Could not start the page engine (WebView2): {err}. \
                         Install the WebView2 Runtime from Microsoft and restart Aurora."
                    ),
                    folder: None,
                });
                cx.notify();
                return;
            }
        };

        let index = if self.tabs.is_empty() { 0 } else { self.active + 1 };
        let index = index.min(self.tabs.len());
        if !activate {
            tab.webview.update(cx, |w, _| w.hide());
        }
        self.tabs.insert(index, tab);
        if activate {
            self.activate(index, window, cx);
        } else if index <= self.active && self.tabs.len() > 1 {
            self.active += 1;
        }
        if activate && address::is_new_tab(url) {
            self.focus_address(window, cx);
        }
        self.save_session();
        cx.notify();
    }

    fn activate(&mut self, index: usize, window: &mut Window, cx: &mut Context<Self>) {
        if index >= self.tabs.len() {
            return;
        }
        for (i, tab) in self.tabs.iter().enumerate() {
            if i != index {
                tab.webview.update(cx, |w, _| w.hide());
            }
        }
        self.active = index;
        let tab = &self.tabs[index];
        tab.webview.update(cx, |w, _| w.show());
        let url = address::display(&tab.url);
        self.address.update(cx, |input, cx| input.set_value(url, window, cx));
        self.update_window_title(window);
        if !address::is_new_tab(&tab.url) {
            let _ = tab.webview.read(cx).raw().focus();
        }
        self.save_session();
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

    /// Ctrl+1..8 picks that tab, Ctrl+9 the last one, as in other browsers.
    fn select_tab_number(&mut self, n: usize, window: &mut Window, cx: &mut Context<Self>) {
        let index = if n == 9 { self.tabs.len().saturating_sub(1) } else { n - 1 };
        self.activate(index, window, cx);
    }

    // ----- navigation -----------------------------------------------------

    fn navigate(&mut self, input: &str, window: &mut Window, cx: &mut Context<Self>) {
        let Some(url) = address::to_url(input) else {
            return;
        };
        let Some(tab) = self.tabs.get_mut(self.active) else {
            return;
        };
        tab.url = url.clone();
        tab.loading = true;
        let raw = tab.webview.read(cx).raw();
        let _ = raw.load_url(&url);
        let _ = raw.focus();
        self.address
            .update(cx, |input, cx| input.set_value(address::display(&url), window, cx));
        cx.notify();
    }

    fn focus_address(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.address.update(cx, |input, cx| {
            input.focus(window, cx);
            input.select_all(window, cx);
        });
    }

    fn toggle_bookmark(&mut self, cx: &mut Context<Self>) {
        let Some(tab) = self.active_tab() else { return };
        if address::is_new_tab(&tab.url) || tab.url.is_empty() {
            return;
        }
        let (url, title) = (tab.url.clone(), tab.display_title());
        {
            let mut bookmarks = self.bookmarks.borrow_mut();
            if let Some(pos) = bookmarks.iter().position(|b| b.url == url) {
                bookmarks.remove(pos);
            } else {
                bookmarks.push(Bookmark { title, url });
            }
            storage::save_bookmarks(&bookmarks);
        }
        cx.notify();
    }

    fn is_bookmarked(&self, url: &str) -> bool {
        self.bookmarks.borrow().iter().any(|b| b.url == url)
    }

    fn save_session(&self) {
        let tabs = self.tabs.iter().map(|t| t.url.clone()).collect();
        storage::save_session(&Session { tabs, active: self.active });
    }

    fn update_window_title(&self, window: &mut Window) {
        let title = match self.active_tab() {
            Some(tab) => format!("{} - Aurora", tab.display_title()),
            None => "Aurora".into(),
        };
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
            }
            InputEvent::Blur => {
                // Put the real address back if the user typed and walked away.
                if let Some(url) = self.active_tab().map(|t| address::display(&t.url)) {
                    input.update(cx, |input, cx| input.set_value(url, window, cx));
                }
            }
            InputEvent::Change => {}
        }
    }

    fn on_page_event(&mut self, id: TabId, event: PageEvent, window: &mut Window, cx: &mut Context<Self>) {
        let Some(index) = self.tab_index(id) else { return };
        let is_active = index == self.active;
        let mut url_changed = false;
        match event {
            PageEvent::TitleChanged(title) => self.tabs[index].title = title,
            PageEvent::LoadStarted(url) => {
                let tab = &mut self.tabs[index];
                tab.loading = true;
                url_changed = tab.url != url;
                tab.url = url;
            }
            PageEvent::LoadFinished(url) => {
                let tab = &mut self.tabs[index];
                tab.loading = false;
                url_changed = tab.url != url;
                tab.url = url;
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
                self.on_shortcut(shortcut, window, cx);
                return;
            }
            PageEvent::DownloadStarted(path) => {
                let name = file_name(Some(&path));
                self.notice = Some(Notice { text: format!("Downloading {name}…"), folder: None });
            }
            PageEvent::DownloadFinished { path, success } => {
                let name = file_name(path.as_ref());
                self.notice = Some(if success {
                    Notice {
                        text: format!("Downloaded {name}"),
                        folder: path
                            .and_then(|p| p.parent().map(PathBuf::from))
                            .or_else(|| Some(storage::downloads_dir())),
                    }
                } else {
                    Notice { text: format!("Download failed: {name}"), folder: None }
                });
            }
        }

        if url_changed {
            self.save_session();
        }
        if is_active {
            let editing = self.address.read(cx).focus_handle(cx).is_focused(window);
            if url_changed && !editing {
                let url = address::display(&self.tabs[index].url);
                self.address.update(cx, |input, cx| input.set_value(url, window, cx));
            }
            self.update_window_title(window);
        }
        cx.notify();
    }

    fn on_shortcut(&mut self, shortcut: Shortcut, window: &mut Window, cx: &mut Context<Self>) {
        match shortcut {
            Shortcut::NewTab => self.open_tab(address::NEW_TAB_URL, true, window, cx),
            Shortcut::CloseTab => self.close_tab(self.active, window, cx),
            Shortcut::ReopenClosedTab => self.reopen_closed_tab(window, cx),
            Shortcut::NextTab => self.cycle_tab(1, window, cx),
            Shortcut::PrevTab => self.cycle_tab(-1, window, cx),
            Shortcut::SelectTab(n) => self.select_tab_number(n, window, cx),
            Shortcut::FocusAddress => {
                // Move keyboard focus from the native page back to GPUI first.
                if let Some(tab) = self.active_tab() {
                    let _ = tab.webview.read(cx).raw().focus_parent();
                }
                self.focus_address(window, cx);
            }
            Shortcut::ToggleBookmark => self.toggle_bookmark(cx),
        }
    }

    fn reopen_closed_tab(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if let Some(url) = self.closed.pop() {
            self.open_tab(&url, true, window, cx);
        }
    }

    // ----- rendering ------------------------------------------------------

    fn render_tab_strip(&self, cx: &mut Context<Self>) -> impl IntoElement {
        let theme = cx.theme();
        let (active_bg, hover_bg, fg, muted) =
            (theme.background, theme.secondary_hover, theme.foreground, theme.muted_foreground);

        h_flex()
            .id("tabs")
            .h_full()
            .flex_1()
            .min_w_0()
            .items_end()
            .gap_1()
            .pl_1()
            .overflow_x_hidden()
            .children(self.tabs.iter().enumerate().map(|(i, tab)| {
                let selected = i == self.active;
                let title = tab.display_title();
                h_flex()
                    .id(("tab", tab.id))
                    .group("tab")
                    .h(px(30.))
                    .min_w(px(60.))
                    .max_w(px(220.))
                    .flex_1()
                    .px_2()
                    .gap_1p5()
                    .items_center()
                    .rounded_t_md()
                    .text_sm()
                    .cursor_pointer()
                    .when(selected, |el| el.bg(active_bg).text_color(fg))
                    .when(!selected, |el| el.text_color(muted).hover(|el| el.bg(hover_bg)))
                    .on_click(cx.listener(move |this, _, window, cx| this.activate(i, window, cx)))
                    .on_mouse_down(
                        MouseButton::Middle,
                        cx.listener(move |this, _, window, cx| this.close_tab(i, window, cx)),
                    )
                    .child(if tab.loading {
                        Icon::new(IconName::LoaderCircle)
                            .size_3p5()
                            .flex_shrink_0()
                            .with_animation(
                                ("spin", tab.id),
                                Animation::new(std::time::Duration::from_millis(900)).repeat(),
                                |icon, t| icon.transform(Transformation::rotate(percentage(t))),
                            )
                            .into_any_element()
                    } else {
                        Icon::new(IconName::Globe).size_3p5().flex_shrink_0().into_any_element()
                    })
                    .child(div().flex_1().min_w_0().truncate().child(title))
                    .child(
                        Button::new(("close-tab", tab.id))
                            .ghost()
                            .xsmall()
                            .icon(IconName::Close)
                            .tooltip("Close tab (Ctrl+W)")
                            .on_click(cx.listener(move |this, _, window, cx| {
                                cx.stop_propagation();
                                this.close_tab(i, window, cx);
                            })),
                    )
            }))
            .child(
                div().pb_1().child(
                    Button::new("new-tab")
                        .ghost()
                        .small()
                        .icon(IconName::Plus)
                        .tooltip("New tab (Ctrl+T)")
                        .on_click(cx.listener(|this, _, window, cx| {
                            this.open_tab(address::NEW_TAB_URL, true, window, cx)
                        })),
                ),
            )
    }

    fn render_toolbar(&self, cx: &mut Context<Self>) -> impl IntoElement {
        let (can_back, can_forward) = self
            .raw(cx, |w| (native::can_go_back(w), native::can_go_forward(w)))
            .unwrap_or((false, false));
        let tab = self.active_tab();
        let loading = tab.is_some_and(|t| t.loading);
        let url = tab.map(|t| t.url.clone()).unwrap_or_default();
        let bookmarked = self.is_bookmarked(&url);
        let can_bookmark = !url.is_empty() && !address::is_new_tab(&url);

        let site_icon: AnyElement = if address::is_new_tab(&url) || url.is_empty() {
            Icon::new(IconName::Search).size_4().into_any_element()
        } else if address::is_secure(&url) {
            Icon::new(Icons::Lock).size_4().into_any_element()
        } else {
            Icon::new(IconName::Info).size_4().into_any_element()
        };

        h_flex()
            .h(px(44.))
            .px_2()
            .gap_1()
            .items_center()
            .bg(cx.theme().background)
            .child(
                Button::new("back")
                    .ghost()
                    .icon(IconName::ArrowLeft)
                    .tooltip("Back (Alt+Left)")
                    .disabled(!can_back)
                    .on_click(cx.listener(|this, _, w, cx| this.go_back(&GoBack, w, cx))),
            )
            .child(
                Button::new("forward")
                    .ghost()
                    .icon(IconName::ArrowRight)
                    .tooltip("Forward (Alt+Right)")
                    .disabled(!can_forward)
                    .on_click(cx.listener(|this, _, w, cx| this.go_forward(&GoForward, w, cx))),
            )
            .child(if loading {
                Button::new("stop")
                    .ghost()
                    .icon(IconName::Close)
                    .tooltip("Stop loading")
                    .on_click(cx.listener(|this, _, _, cx| {
                        this.raw(cx, native::stop);
                        if let Some(tab) = this.tabs.get_mut(this.active) {
                            tab.loading = false;
                        }
                        cx.notify();
                    }))
            } else {
                Button::new("reload")
                    .ghost()
                    .icon(IconName::RotateCw)
                    .tooltip("Reload (F5)")
                    .on_click(cx.listener(|this, _, w, cx| this.reload(&Reload, w, cx)))
            })
            .child(
                Button::new("home")
                    .ghost()
                    .icon(Icons::House)
                    .tooltip("New tab page (Alt+Home)")
                    .on_click(cx.listener(|this, _, w, cx| this.go_home(&GoHome, w, cx))),
            )
            .child(
                div().flex_1().min_w_0().px_1().child(
                    Input::new(&self.address)
                        .prefix(div().text_color(cx.theme().muted_foreground).child(site_icon))
                        .suffix(
                            Button::new("bookmark")
                                .ghost()
                                .xsmall()
                                .icon(if bookmarked { IconName::StarFill } else { IconName::Star })
                                .tooltip(if bookmarked {
                                    "Remove bookmark (Ctrl+D)"
                                } else {
                                    "Bookmark this page (Ctrl+D)"
                                })
                                .disabled(!can_bookmark)
                                .on_click(cx.listener(|this, _, _, cx| this.toggle_bookmark(cx))),
                        ),
                ),
            )
            .child(
                Button::new("devtools")
                    .ghost()
                    .icon(IconName::SquareTerminal)
                    .tooltip("Developer tools (F12)")
                    .on_click(cx.listener(|this, _, w, cx| this.open_devtools(&OpenDevTools, w, cx))),
            )
    }

    fn render_bookmarks_bar(&self, cx: &mut Context<Self>) -> impl IntoElement {
        let bookmarks = self.bookmarks.borrow();
        h_flex()
            .id("bookmarks")
            .h(px(30.))
            .px_2()
            .gap_0p5()
            .items_center()
            .overflow_x_hidden()
            .bg(cx.theme().background)
            .border_b_1()
            .border_color(cx.theme().border)
            .when(bookmarks.is_empty(), |el| {
                el.child(
                    div()
                        .px_2()
                        .text_xs()
                        .text_color(cx.theme().muted_foreground)
                        .child("Press Ctrl+D or the star to bookmark a page."),
                )
            })
            .children(bookmarks.iter().enumerate().map(|(i, b)| {
                let url = b.url.clone();
                Button::new(("bookmark", i))
                    .ghost()
                    .xsmall()
                    .icon(IconName::Globe)
                    .label(truncate(&b.title, 24))
                    .tooltip(b.url.clone())
                    .on_click(cx.listener(move |this, _, window, cx| this.navigate(&url, window, cx)))
            }))
    }

    fn render_notice(&self, cx: &mut Context<Self>) -> Option<impl IntoElement + use<>> {
        let notice = self.notice.as_ref()?;
        let folder = notice.folder.clone();
        Some(
            h_flex()
                .h(px(32.))
                .px_3()
                .gap_2()
                .items_center()
                .text_sm()
                .bg(cx.theme().secondary)
                .border_b_1()
                .border_color(cx.theme().border)
                .child(Icon::new(Icons::Download).size_4())
                .child(div().flex_1().min_w_0().truncate().child(notice.text.clone()))
                .when_some(folder, |el, folder| {
                    el.child(
                        Button::new("open-folder")
                            .ghost()
                            .xsmall()
                            .label("Show in folder")
                            .on_click(move |_, _, _| open_folder(&folder)),
                    )
                })
                .child(
                    Button::new("dismiss-notice")
                        .ghost()
                        .xsmall()
                        .icon(IconName::Close)
                        .on_click(cx.listener(|this, _, _, cx| {
                            this.notice = None;
                            cx.notify();
                        })),
                ),
        )
    }

    // ----- action handlers ------------------------------------------------

    fn new_tab(&mut self, _: &NewTab, window: &mut Window, cx: &mut Context<Self>) {
        self.on_shortcut(Shortcut::NewTab, window, cx);
    }
    fn close_active_tab(&mut self, _: &CloseTab, window: &mut Window, cx: &mut Context<Self>) {
        self.on_shortcut(Shortcut::CloseTab, window, cx);
    }
    fn reopen_tab(&mut self, _: &ReopenClosedTab, window: &mut Window, cx: &mut Context<Self>) {
        self.on_shortcut(Shortcut::ReopenClosedTab, window, cx);
    }
    fn next_tab(&mut self, _: &NextTab, window: &mut Window, cx: &mut Context<Self>) {
        self.on_shortcut(Shortcut::NextTab, window, cx);
    }
    fn prev_tab(&mut self, _: &PrevTab, window: &mut Window, cx: &mut Context<Self>) {
        self.on_shortcut(Shortcut::PrevTab, window, cx);
    }
    fn focus_address_action(&mut self, _: &FocusAddress, window: &mut Window, cx: &mut Context<Self>) {
        self.focus_address(window, cx);
    }
    fn bookmark_action(&mut self, _: &ToggleBookmark, _: &mut Window, cx: &mut Context<Self>) {
        self.toggle_bookmark(cx);
    }
    fn reload(&mut self, _: &Reload, _: &mut Window, cx: &mut Context<Self>) {
        self.raw(cx, |w| {
            let _ = w.reload();
        });
    }
    fn go_back(&mut self, _: &GoBack, _: &mut Window, cx: &mut Context<Self>) {
        self.raw(cx, native::go_back);
    }
    fn go_forward(&mut self, _: &GoForward, _: &mut Window, cx: &mut Context<Self>) {
        self.raw(cx, native::go_forward);
    }
    fn go_home(&mut self, _: &GoHome, window: &mut Window, cx: &mut Context<Self>) {
        self.navigate(address::NEW_TAB_URL, window, cx);
        self.focus_address(window, cx);
    }
    fn open_devtools(&mut self, _: &OpenDevTools, _: &mut Window, cx: &mut Context<Self>) {
        self.raw(cx, |w| w.open_devtools());
    }
}

impl Focusable for Browser {
    fn focus_handle(&self, _: &App) -> FocusHandle {
        self.focus_handle.clone()
    }
}

impl Render for Browser {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let page = self.active_tab().map(|t| t.webview.clone());
        let notice = self.render_notice(cx);

        v_flex()
            .size_full()
            .track_focus(&self.focus_handle)
            .bg(cx.theme().background)
            .text_color(cx.theme().foreground)
            .on_action(cx.listener(Self::new_tab))
            .on_action(cx.listener(Self::close_active_tab))
            .on_action(cx.listener(Self::reopen_tab))
            .on_action(cx.listener(Self::next_tab))
            .on_action(cx.listener(Self::prev_tab))
            .on_action(cx.listener(Self::focus_address_action))
            .on_action(cx.listener(Self::bookmark_action))
            .on_action(cx.listener(Self::reload))
            .on_action(cx.listener(Self::go_back))
            .on_action(cx.listener(Self::go_forward))
            .on_action(cx.listener(Self::go_home))
            .on_action(cx.listener(Self::open_devtools))
            .child(TitleBar::new().child(self.render_tab_strip(cx)))
            .child(self.render_toolbar(cx))
            .child(self.render_bookmarks_bar(cx))
            .children(notice)
            .child(div().flex_1().min_h_0().w_full().children(page))
    }
}

fn truncate(s: &str, max: usize) -> String {
    if s.chars().count() <= max {
        s.to_string()
    } else {
        let mut out: String = s.chars().take(max - 1).collect();
        out.push('…');
        out
    }
}

fn file_name(path: Option<&PathBuf>) -> String {
    path.and_then(|p| p.file_name())
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_else(|| "file".into())
}

fn open_folder(folder: &std::path::Path) {
    #[cfg(windows)]
    let _ = std::process::Command::new("explorer").arg(folder).spawn();
    #[cfg(not(windows))]
    let _ = folder;
}
