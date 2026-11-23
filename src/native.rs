//! Engine calls that wry does not wrap. On Windows these go straight to
//! WebView2.

/// Optional detail a page engine reports after a navigation. Providers such
/// as WebView2 send it a moment after the title; engines without it report
/// `None`.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum NavigationStatus {
    Success,
    Failed,
}

/// Events from WebView2 handlers registered here.
pub enum NativeEvent {
    /// A page element (usually a video) entered or left fullscreen.
    Fullscreen(bool),
    /// The page started or stopped playing sound.
    Audio(bool),
}

#[cfg(windows)]
mod imp {
    use webview2_com::Microsoft::Web::WebView2::Win32::{
        ICoreWebView2_8, ICoreWebView2NavigationCompletedEventArgs, ICoreWebView2Settings4,
    };
    use webview2_com::{
        ContainsFullScreenElementChangedEventHandler, IsDocumentPlayingAudioChangedEventHandler,
        NavigationCompletedEventHandler,
    };
    use windows_core::{BOOL, Interface as _};
    use wry::WebViewExtWindows as _;

    use super::{NativeEvent, NavigationStatus};

    // SAFETY (whole module): every call is a method on a live COM interface
    // owned by the wry WebView passed in; out pointers are valid locals.

    pub fn can_go_back(webview: &wry::WebView) -> bool {
        let mut value = BOOL::default();
        unsafe { webview.webview().CanGoBack(&mut value) }.is_ok() && value.as_bool()
    }

    pub fn can_go_forward(webview: &wry::WebView) -> bool {
        let mut value = BOOL::default();
        unsafe { webview.webview().CanGoForward(&mut value) }.is_ok() && value.as_bool()
    }

    pub fn go_back(webview: &wry::WebView) {
        let _ = unsafe { webview.webview().GoBack() };
    }

    pub fn go_forward(webview: &wry::WebView) {
        let _ = unsafe { webview.webview().GoForward() };
    }

    pub fn stop(webview: &wry::WebView) {
        let _ = unsafe { webview.webview().Stop() };
    }

    /// Whether the page that just finished loading rendered successfully, so
    /// DNS errors and failed connections can stop the loading indicator that
    /// the engine's own load callback never completes for them.
    pub fn last_navigation(webview: &wry::WebView) -> Option<NavigationStatus> {
        let storage = std::sync::Arc::new(std::sync::Mutex::new(None::<NavigationStatus>));
        let handler = NavigationCompletedEventHandler::create(Box::new({
            let storage = storage.clone();
            move |_, args: Option<ICoreWebView2NavigationCompletedEventArgs>| {
                let Some(args) = args else {
                    return Ok(());
                };
                let mut success = BOOL::default();
                if unsafe { args.IsSuccess(&mut success) }.is_ok() {
                    if let Ok(mut slot) = storage.lock() {
                        *slot = Some(if success.as_bool() { NavigationStatus::Success } else { NavigationStatus::Failed });
                    }
                }
                Ok(())
            }
        }));
        let mut token = 0i64;
        if unsafe { webview.webview().add_NavigationCompleted(&handler, &mut token) }.is_err() {
            return None;
        }
        let status = storage.lock().ok().and_then(|slot| *slot);
        let _ = unsafe { webview.webview().remove_NavigationCompleted(token) };
        status
    }

    pub fn set_muted(webview: &wry::WebView, muted: bool) {
        if let Ok(wv) = webview.webview().cast::<ICoreWebView2_8>() {
            let _ = unsafe { wv.SetIsMuted(muted) };
        }
    }

    /// Turns WebView2's built-in password manager and form autofill on or off.
    pub fn set_save_passwords(webview: &wry::WebView, enabled: bool) {
        let settings = unsafe { webview.webview().Settings() };
        if let Ok(settings) = settings.and_then(|s| s.cast::<ICoreWebView2Settings4>()) {
            let _ = unsafe { settings.SetIsPasswordAutosaveEnabled(enabled) };
            let _ = unsafe { settings.SetIsGeneralAutofillEnabled(enabled) };
        }
    }

    pub fn watch(webview: &wry::WebView, send: impl Fn(NativeEvent) + Clone + 'static) {
        let core = webview.webview();
        let mut token = 0i64;

        let on_fullscreen = send.clone();
        let handler = ContainsFullScreenElementChangedEventHandler::create(Box::new(move |sender, _| {
            if let Some(sender) = sender {
                let mut value = BOOL::default();
                unsafe { sender.ContainsFullScreenElement(&mut value)? };
                on_fullscreen(NativeEvent::Fullscreen(value.as_bool()));
            }
            Ok(())
        }));
        let _ = unsafe { core.add_ContainsFullScreenElementChanged(&handler, &mut token) };

        if let Ok(core8) = core.cast::<ICoreWebView2_8>() {
            let handler = IsDocumentPlayingAudioChangedEventHandler::create(Box::new(move |sender, _| {
                if let Some(sender) = sender.and_then(|s| s.cast::<ICoreWebView2_8>().ok()) {
                    let mut value = BOOL::default();
                    unsafe { sender.IsDocumentPlayingAudio(&mut value)? };
                    send(NativeEvent::Audio(value.as_bool()));
                }
                Ok(())
            }));
            let _ = unsafe { core8.add_IsDocumentPlayingAudioChanged(&handler, &mut token) };
        }
    }
}

#[cfg(not(windows))]
mod imp {
    use super::{NativeEvent, NavigationStatus};

    pub fn last_navigation(_: &wry::WebView) -> Option<NavigationStatus> {
        None
    }

    pub fn can_go_back(_: &wry::WebView) -> bool {
        true
    }
    pub fn can_go_forward(_: &wry::WebView) -> bool {
        true
    }
    pub fn go_back(webview: &wry::WebView) {
        let _ = webview.evaluate_script("history.back()");
    }
    pub fn go_forward(webview: &wry::WebView) {
        let _ = webview.evaluate_script("history.forward()");
    }
    pub fn stop(webview: &wry::WebView) {
        let _ = webview.evaluate_script("window.stop()");
    }
    pub fn set_muted(_: &wry::WebView, _: bool) {}
    pub fn set_save_passwords(_: &wry::WebView, _: bool) {}
    pub fn watch(_: &wry::WebView, _: impl Fn(NativeEvent) + Clone + 'static) {}
}

pub use imp::*;
