// No console window behind the browser in release builds.
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

mod address;
mod browser;
mod extensions;
mod favicons;
mod import;
mod library;
mod native;
mod pages;
mod settings;
mod storage;
mod tab;
mod themes;
mod update;

fn main() {
    // gpui-webview needs this on Windows, or the WebView2 page is never drawn.
    #[cfg(target_os = "windows")]
    // SAFETY: set before any other thread exists.
    unsafe {
        std::env::set_var("GPUI_DISABLE_DIRECT_COMPOSITION", "true");
    }

    // `aurora <url> [url...]` opens those pages instead of restoring the last session.
    let args: Vec<String> = std::env::args().skip(1).filter(|a| !a.starts_with('-')).collect();

    #[cfg(target_os = "linux")]
    let app = gpui_kit::platform::linux(gpui_kit::WindowingModes::X11);
    #[cfg(not(target_os = "linux"))]
    let app = gpui_kit::application();

    app.with_assets(gpui_kit::assets::AllAssets).run(move |cx| {
        gpui_kit::init(cx);
        browser::bind_keys(cx);
        let lib = library::init(cx);
        library::apply_theme(cx);

        let (restore, search) = {
            let lib = lib.borrow();
            (lib.settings.restore_session, settings::search_engine(&lib.settings.search_engine).url)
        };
        let urls: Vec<String> = args.iter().filter_map(|a| address::to_url(a, search)).collect();
        if !urls.is_empty() {
            browser::open_window(urls, 0, false, cx);
        } else {
            let windows = if restore { storage::load_session() } else { Vec::new() };
            if windows.is_empty() {
                browser::open_window(Vec::new(), 0, false, cx);
            }
            for w in windows {
                browser::open_window(w.tabs, w.active, false, cx);
            }
        }

        cx.on_window_closed(|cx, _| {
            if cx.windows().is_empty() {
                cx.quit();
            }
        })
        .detach();
    });
}
