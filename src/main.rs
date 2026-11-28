// No console window behind the browser. This is set in every build, not just
// release ones: a debug build that opened a console beside the window also
// died with it when the console was closed. Output goes to `chrust.log`.
#![cfg_attr(target_os = "windows", windows_subsystem = "windows")]

mod address;
mod adblock;
mod browser;
mod crashlog;
mod engine;
mod extensions;
mod favicons;
mod import;
mod library;
mod native;
mod pages;
mod perf;
mod proxy;
mod settings;
mod signin;
mod storage;
mod tab;
mod themes;
mod update;
mod youtube;

fn main() {
    // First thing, so every later timing is measured from here.
    perf::start();

    // gpui-webview needs this on Windows, or the WebView2 page is never drawn.
    #[cfg(target_os = "windows")]
    // SAFETY: set before any other thread exists.
    unsafe {
        std::env::set_var("GPUI_DISABLE_DIRECT_COMPOSITION", "true");
    }

    let log_file = crashlog::start();
    std::panic::set_hook(Box::new(move |info| {
        let where_ = info.location().map(|l| format!("{}:{}", l.file(), l.line())).unwrap_or_default();
        log::error!("panic at {where_}: {info}");
        // `RUST_BACKTRACE=1` turns the capture into real frames; it is off by
        // default, so a normal run pays nothing for it.
        log::error!("backtrace: {}", std::backtrace::Backtrace::capture());
        // Nothing here needs to keep running after a panic, and the log has
        // the details.
        std::process::exit(101);
    }));

    // `chrust <url> [url...]` opens those pages instead of restoring the last session.
    let args: Vec<String> = std::env::args().skip(1).filter(|a| !a.starts_with('-')).collect();

    // Start compiling the ad blocking rules while the window is still being
    // built, so the first page opened already has an engine to ask. Upstream
    // lists are fetched in the same background build when the setting is on.
    adblock::warm(storage::load_settings().adblock_upstream);

    #[cfg(target_os = "linux")]
    let app = gpui_kit::platform::linux(gpui_kit::WindowingModes::X11);
    #[cfg(not(target_os = "linux"))]
    let app = gpui_kit::application();

    app.with_assets(gpui_kit::assets::AllAssets).run(move |cx| {
        gpui_kit::init(cx);
        browser::bind_keys(cx);
        let lib = library::init(cx);
        library::apply_theme(cx);
        log::info!("Chrust {} started, log: {:?}", env!("CARGO_PKG_VERSION"), log_file);
        perf::mark("interface set up");

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
        perf::mark("windows opened");

        cx.on_window_closed(|cx, _| {
            if cx.windows().is_empty() {
                cx.quit();
            }
        })
        .detach();
    });
}
