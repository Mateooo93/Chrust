//! Engine calls that wry does not wrap. On Windows these go straight to
//! WebView2, which also works on error pages where `history.back()` cannot run.

#[cfg(windows)]
mod imp {
    use wry::WebViewExtWindows as _;

    pub fn can_go_back(webview: &wry::WebView) -> bool {
        let mut value = windows_core::BOOL::default();
        // SAFETY: `webview()` returns a live ICoreWebView2 owned by `webview`,
        // and `value` is a valid out pointer for the duration of the call.
        unsafe { webview.webview().CanGoBack(&mut value) }.is_ok() && value.as_bool()
    }

    pub fn can_go_forward(webview: &wry::WebView) -> bool {
        let mut value = windows_core::BOOL::default();
        // SAFETY: as in `can_go_back`.
        unsafe { webview.webview().CanGoForward(&mut value) }.is_ok() && value.as_bool()
    }

    pub fn go_back(webview: &wry::WebView) {
        // SAFETY: plain method call on a live COM interface.
        let _ = unsafe { webview.webview().GoBack() };
    }

    pub fn go_forward(webview: &wry::WebView) {
        // SAFETY: plain method call on a live COM interface.
        let _ = unsafe { webview.webview().GoForward() };
    }

    pub fn stop(webview: &wry::WebView) {
        // SAFETY: plain method call on a live COM interface.
        let _ = unsafe { webview.webview().Stop() };
    }
}

#[cfg(not(windows))]
mod imp {
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
}

pub use imp::*;
