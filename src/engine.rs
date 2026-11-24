//! The page engine: one WebView2 environment for the whole browser.
//!
//! Making the environment finds the WebView2 runtime and starts its browser
//! process, which is the slowest part of opening a page. wry makes one for
//! every page, so each tab pays for the engine before it pays for the page.
//! Chrust keeps the environment the first page started and hands it to every
//! page after that, so they join the engine process that is already running.
//!
//! Everything here stays on the window thread: a WebView2 object belongs to the
//! thread that made it, and cannot be handed to another one.

#[cfg(windows)]
pub use imp::*;
#[cfg(not(windows))]
pub use other::*;

#[cfg(windows)]
mod imp {
    use std::cell::{Cell, RefCell};

    use webview2_com::Microsoft::Web::WebView2::Win32::ICoreWebView2Environment;

    thread_local! {
        /// The environment the first page started, reused by every page after
        /// it. Thread-local because it only means anything on the thread that
        /// made it.
        static ENVIRONMENT: RefCell<Option<ICoreWebView2Environment>> = const { RefCell::new(None) };
        /// Set when a page would not start with the shared environment: from
        /// then on each page starts its own, which is what wry does anyway.
        static REFUSED: Cell<bool> = const { Cell::new(false) };
    }

    /// The Chromium switches every page is created with, including the proxy
    /// when one is configured. wry passes these to the environment it builds,
    /// so every page that shares it gets the same engine settings.
    pub fn browser_args() -> String {
        let mut args = String::from(
            "--enable-features=msExtensions,msEdgeExtensions \
             --disable-features=msWebOOUI,msPdfOOUI,msSmartScreenProtection",
        );
        if let Some(proxy) = crate::proxy::configured() {
            args.push(' ');
            args.push_str(&proxy.browser_flag());
        }
        args
    }

    /// Keeps the environment a page just started, so later pages share it.
    pub fn remember(webview: &wry::WebView) {
        use wry::WebViewExtWindows as _;
        if REFUSED.with(|refused| refused.get()) {
            return;
        }
        ENVIRONMENT.with(|slot| {
            if slot.borrow().is_none() {
                *slot.borrow_mut() = Some(webview.environment());
            }
        });
    }

    /// Gives up on the shared environment after a page would not start with
    /// it; true only the first time, so the caller can try once more without
    /// it.
    pub fn refuse() -> bool {
        let first = !REFUSED.with(|refused| refused.replace(true));
        ENVIRONMENT.with(|slot| *slot.borrow_mut() = None);
        first
    }

    /// Gives the builder the shared environment, if there is one yet.
    pub fn attach(builder: wry::WebViewBuilder) -> wry::WebViewBuilder {
        use wry::WebViewBuilderExtWindows as _;
        match ENVIRONMENT.with(|slot| slot.borrow().clone()) {
            Some(environment) => builder.with_environment(environment),
            None => builder,
        }
    }
}

#[cfg(not(windows))]
mod other {
    /// Engines elsewhere take their start-up arguments in the builder.
    pub fn browser_args() -> String {
        String::new()
    }

    /// Those engines share what they can already.
    pub fn remember(_: &wry::WebView) {}

    /// Nothing is shared here, so there is nothing to give up on.
    pub fn refuse() -> bool {
        false
    }

    /// wry's builder already does the right thing.
    pub fn attach(builder: wry::WebViewBuilder) -> wry::WebViewBuilder {
        builder
    }
}
