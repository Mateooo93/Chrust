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
    /// A top-level navigation finished: the address it ended at, and whether
    /// it rendered. Failed navigations (DNS errors, refused connections) only
    /// show up here.
    Navigation(String, NavigationStatus),
}

#[cfg(windows)]
mod imp {
    use webview2_com::Microsoft::Web::WebView2::Win32::{
        ICoreWebView2, ICoreWebView2_8, ICoreWebView2NavigationCompletedEventArgs,
        ICoreWebView2Settings4,
    };
    use webview2_com::{
        ContainsFullScreenElementChangedEventHandler, IsDocumentPlayingAudioChangedEventHandler,
        NavigationCompletedEventHandler,
    };
    use webview2_com::take_pwstr;
    use windows_core::{BOOL, Interface as _, PWSTR};
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

    /// Whether the engine accepted the move; it refuses when there is no
    /// history entry in that direction.
    pub fn go_back(webview: &wry::WebView) -> bool {
        unsafe { webview.webview().GoBack() }.is_ok()
    }

    pub fn go_forward(webview: &wry::WebView) -> bool {
        unsafe { webview.webview().GoForward() }.is_ok()
    }

    pub fn stop(webview: &wry::WebView) {
        let _ = unsafe { webview.webview().Stop() };
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
            let on_audio = send.clone();
            let handler = IsDocumentPlayingAudioChangedEventHandler::create(Box::new(move |sender, _| {
                if let Some(sender) = sender.and_then(|s| s.cast::<ICoreWebView2_8>().ok()) {
                    let mut value = BOOL::default();
                    unsafe { sender.IsDocumentPlayingAudio(&mut value)? };
                    on_audio(NativeEvent::Audio(value.as_bool()));
                }
                Ok(())
            }));
            let _ = unsafe { core8.add_IsDocumentPlayingAudioChanged(&handler, &mut token) };
        }

        // Registered for the life of the page. wry's load callback says a load
        // finished either way; only this says whether the page actually
        // rendered, which is what ends the spinner on a DNS error or a
        // refused connection.
        let on_navigation = send;
        let handler = NavigationCompletedEventHandler::create(Box::new(
            move |sender: Option<ICoreWebView2>, args: Option<ICoreWebView2NavigationCompletedEventArgs>| {
                let (Some(sender), Some(args)) = (sender, args) else {
                    return Ok(());
                };
                let mut success = BOOL::default();
                let status = match unsafe { args.IsSuccess(&mut success) } {
                    Ok(()) if success.as_bool() => NavigationStatus::Success,
                    _ => NavigationStatus::Failed,
                };
                let mut address = PWSTR::null();
                let url = match unsafe { sender.Source(&mut address) } {
                    Ok(()) => take_pwstr(address),
                    Err(_) => String::new(),
                };
                on_navigation(NativeEvent::Navigation(url, status));
                Ok(())
            },
        ));
        let _ = unsafe { core.add_NavigationCompleted(&handler, &mut token) };
    }
}

#[cfg(not(windows))]
mod imp {
    use super::{NativeEvent, NavigationStatus};

    pub fn can_go_back(webview: &wry::WebView) -> bool {
        true
    }
    pub fn can_go_forward(webview: &wry::WebView) -> bool {
        true
    }
    /// Back and forward, then the page reports its new address: on these
    /// engines wry reports load progress for the document only, and a move
    /// between history entries may not load one.
    pub fn go_back(webview: &wry::WebView) -> bool {
        let moved = webview.evaluate_script("history.back()").is_ok();
        report_url(webview);
        moved
    }
    pub fn go_forward(webview: &wry::WebView) -> bool {
        let moved = webview.evaluate_script("history.forward()").is_ok();
        report_url(webview);
        moved
    }

    #[cfg(target_os = "linux")]
    fn report_url(webview: &wry::WebView) {
        let _ = webview.evaluate_script(
            "setTimeout(() => { try { window.ipc.postMessage(JSON.stringify({ kind: 'url' })) } catch (_) {} }, 250)",
        );
    }
    #[cfg(not(target_os = "linux"))]
    fn report_url(_: &wry::WebView) {}
    pub fn stop(webview: &wry::WebView) {
        let _ = webview.evaluate_script("window.stop()");
    }
    pub fn set_muted(_: &wry::WebView, _: bool) {}
    pub fn set_save_passwords(_: &wry::WebView, _: bool) {}
    pub fn watch(_: &wry::WebView, _: impl Fn(NativeEvent) + Clone + 'static) {}

}

pub use imp::*;
