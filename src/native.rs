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
    use std::cell::RefCell;
    use std::rc::Rc;

    use webview2_com::Microsoft::Web::WebView2::Win32::{
        ICoreWebView2, ICoreWebView2Environment, ICoreWebView2NavigationCompletedEventArgs,
        ICoreWebView2Settings4, ICoreWebView2WebResourceRequest,
        ICoreWebView2WebResourceRequestedEventArgs, ICoreWebView2WebResourceResponse,
        ICoreWebView2_22, ICoreWebView2_8,
        COREWEBVIEW2_WEB_RESOURCE_CONTEXT_ALL,
        COREWEBVIEW2_WEB_RESOURCE_REQUEST_SOURCE_KINDS_ALL,
    };
    use webview2_com::{
        take_pwstr, ContainsFullScreenElementChangedEventHandler,
        IsDocumentPlayingAudioChangedEventHandler, NavigationCompletedEventHandler,
        WebResourceRequestedEventHandler,
    };
    use windows::core::HSTRING;
    use windows_core::{BOOL, Interface as _, PCWSTR, PWSTR};
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

    /// Stops requests the ad blocker says are ads or trackers.
    ///
    /// Every request a page makes comes through WebView2 before it leaves the
    /// machine, including the ones a page script starts, service workers' and
    /// subframes'. A blocked request is answered with an empty body and a
    /// failure status, which is what a browser with its own network stack
    /// does: scripts see a failed fetch, `img` elements fire `error`, and the
    /// page carries on. `private` tabs always block, so the address of the
    /// tab and the address bar cannot disagree.
    pub fn watch_requests(webview: &wry::WebView, private: bool) {
        let core = webview.webview();
        // One filter for every context: images, scripts, fetches, frames,
        // media and the top-level document. The engine decides by request
        // type. The `_22` method also covers shared workers and the like;
        // older runtimes fall back to the method every version has.
        if let Ok(core22) = core.cast::<ICoreWebView2_22>() {
            let _ = unsafe {
                core22.AddWebResourceRequestedFilterWithRequestSourceKinds(
                    &HSTRING::from("*"),
                    COREWEBVIEW2_WEB_RESOURCE_CONTEXT_ALL,
                    COREWEBVIEW2_WEB_RESOURCE_REQUEST_SOURCE_KINDS_ALL,
                )
            };
        } else {
            let _ = unsafe {
                core.AddWebResourceRequestedFilter(&HSTRING::from("*"), COREWEBVIEW2_WEB_RESOURCE_CONTEXT_ALL)
            };
        }

        // The environment makes the response; it is per browser process, and
        // this page's own environment is the shared one (see `engine`). Rc
        // and RefCell because WebView2 calls this on the window thread only.
        let environment = Rc::new(RefCell::new(Some(webview.environment())));

        let mut token = 0i64;
        let handler = WebResourceRequestedEventHandler::create(Box::new(
            move |sender: Option<ICoreWebView2>,
                  args: Option<ICoreWebView2WebResourceRequestedEventArgs>| {
                let (Some(sender), Some(args)) = (sender, args) else { return Ok(()) };
                let Some(url) = (unsafe { request_uri(&args) }) else { return Ok(()) };
                // Not every request can be blocked, and not every resource
                // type has a rule: the engine does it quickly, but a 403 on a
                // WebSocket or a data: URL would break a page for nothing.
                if !is_http(&url) {
                    return Ok(());
                }
                let context = unsafe {
                    let mut context = Default::default();
                    args.ResourceContext(&mut context).map(|()| context).unwrap_or_default()
                };
                let kind = crate::adblock::request_type(context.0);
                // The page that asked. For the document itself this is the
                // address being opened, which is what makes first-party
                // checks (`domain=site`) work for a top-level load.
                let source = unsafe { page_url(&sender) };
                if !crate::adblock::should_block(&url, &source, kind, private) {
                    return Ok(());
                }
                let Some(environment) = environment.borrow().clone() else { return Ok(()) };
                if let Ok(response) = unsafe { blocked_response(&environment) } {
                    let _ = unsafe { args.SetResponse(&response) };
                    log::debug!("adblock: blocked {kind} {url} on {source}");
                } else {
                    log::debug!("adblock: matched {kind} {url} but could not answer");
                }
                Ok(())
            },
        ));
        let _ = unsafe { core.add_WebResourceRequested(&handler, &mut token) };
    }

    /// Only ordinary web requests can be answered with a synthetic response.
    fn is_http(url: &str) -> bool {
        url.starts_with("http://") || url.starts_with("https://")
    }

    /// The URI of a request, or `None` if WebView2 would not give it.
    unsafe fn request_uri(args: &ICoreWebView2WebResourceRequestedEventArgs) -> Option<String> {
        // SAFETY: `args` is a live interface owned by the caller; `Request`
        // hands back an owned reference, and `Uri` fills a string that is
        // released here with the COM allocator.
        let request: ICoreWebView2WebResourceRequest = unsafe { args.Request() }.ok()?;
        let mut uri = PWSTR::null();
        unsafe { request.Uri(&mut uri) }.ok()?;
        if uri.is_null() {
            return None;
        }
        Some(take_pwstr(uri))
    }

    /// The address of the page, which every request of the page names as its
    /// source.
    unsafe fn page_url(webview: &ICoreWebView2) -> String {
        let mut source = PWSTR::null();
        if unsafe { webview.Source(&mut source) }.is_err() || source.is_null() {
            return String::new();
        }
        take_pwstr(source)
    }

    /// A 403 with an empty body: what a browser's own network stack hands a
    /// page for a blocked request. `img` elements fire `error`, `fetch` sees a
    /// failed request, and nothing waits for a response that never comes.
    unsafe fn blocked_response(
        environment: &ICoreWebView2Environment,
    ) -> windows::core::Result<ICoreWebView2WebResourceResponse> {
        let reason = HSTRING::from("Blocked");
        let headers = HSTRING::from("content-length: 0");
        // No body stream: the response is complete with an empty one, and a
        // blocked resource has nothing to hand back.
        unsafe {
            environment.CreateWebResourceResponse(
                None,
                403,
                PCWSTR(reason.as_ptr()),
                PCWSTR(headers.as_ptr()),
            )
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

    pub fn can_go_back(_webview: &wry::WebView) -> bool {
        true
    }
    pub fn can_go_forward(_webview: &wry::WebView) -> bool {
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
    pub fn watch_requests(_: &wry::WebView, _: bool) {}
    pub fn watch(_: &wry::WebView, _: impl Fn(NativeEvent) + Clone + 'static) {}

}

pub use imp::*;
