// No console window behind the browser in release builds.
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

mod address;
mod browser;
mod import;
mod native;
mod pages;
mod storage;
mod tab;

use gpui_kit::component::{Theme, TitleBar};
use gpui_kit::*;

fn main() {
    // gpui-webview needs this on Windows, or the WebView2 page is never drawn.
    #[cfg(target_os = "windows")]
    // SAFETY: set before any other thread exists.
    unsafe {
        std::env::set_var("GPUI_DISABLE_DIRECT_COMPOSITION", "true");
    }

    // `aurora <url> [url...]` opens those pages instead of restoring the last session.
    let start_urls: Vec<String> = std::env::args()
        .skip(1)
        .filter(|a| !a.starts_with('-'))
        .filter_map(|a| address::to_url(&a))
        .collect();

    #[cfg(target_os = "linux")]
    let app = gpui_kit::platform::linux(WindowingModes::X11);
    #[cfg(not(target_os = "linux"))]
    let app = gpui_kit::application();

    app.with_assets(gpui_kit::assets::AllAssets).run(move |cx| {
        gpui_kit::init(cx);
        browser::bind_keys(cx);

        let options = WindowOptions {
            window_bounds: Some(WindowBounds::centered(size(px(1280.), px(840.)), cx)),
            window_min_size: Some(size(px(480.), px(320.))),
            ..TitleBar::window_options()
        };
        let opened = gpui_kit::open_window(options, cx, |window, cx| {
            Theme::sync_system_appearance(Some(window), cx);
            window.set_window_title("Aurora");
            cx.new(|cx| {
                let browser = browser::Browser::new(start_urls, window, cx);
                cx.observe_window_appearance(window, |_, window, cx| {
                    Theme::sync_system_appearance(Some(window), cx);
                })
                .detach();
                browser
            })
        });
        match opened {
            Ok((_, browser)) => {
                cx.on_window_closed(|cx, _| {
                    if cx.windows().is_empty() {
                        cx.quit();
                    }
                })
                .detach();
                let _ = browser;
            }
            Err(err) => {
                eprintln!("Aurora could not open a window: {err:#}");
                cx.quit();
            }
        }
    });
}
