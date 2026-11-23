#![allow(dead_code)]
mod browser;
mod css;
mod dom;
mod fonts;
mod html;
mod js;
mod layout;
mod net;
mod page;
mod paint;
mod style;
mod util;

use fonts::FontStore;
use page::Page;
use paint::{render_page, save_png, Pixmap};
use util::Color;

struct Options {
    url: Option<String>,
    screenshot: Option<String>,
    width: usize,
    height: usize,
    viewport_only: bool,
    dump_text: bool,
    dump_layout: bool,
    click: Option<String>,
}

fn usage() {
    println!(
        "Aurora — a native browser written in Rust\n\
         \n\
         USAGE:\n\
         \x20 aurora [OPTIONS] [URL]\n\
         \n\
         OPTIONS:\n\
         \x20 --screenshot <path>   Render the page headlessly to a PNG\n\
         \x20 --width <px>          Viewport width (default 1280)\n\
         \x20 --height <px>         Viewport height (default 800)\n\
         \x20 --viewport            Screenshot only the viewport, not the full page\n\
         \x20 --dump-text           Print the document's text content\n\
         \x20 -h, --help            Show this help"
    );
}

fn parse_args() -> Options {
    let mut opts = Options {
        url: None,
        screenshot: None,
        width: 1280,
        height: 800,
        viewport_only: false,
        dump_text: false,
        dump_layout: false,
        click: None,
    };
    let args: Vec<String> = std::env::args().skip(1).collect();
    let mut i = 0;
    while i < args.len() {
        let a = &args[i];
        match a.as_str() {
            "-h" | "--help" => {
                usage();
                std::process::exit(0);
            }
            "--screenshot" => {
                i += 1;
                if i < args.len() {
                    opts.screenshot = Some(args[i].clone());
                }
            }
            "--width" => {
                i += 1;
                if i < args.len() {
                    opts.width = args[i].parse().unwrap_or(1280);
                }
            }
            "--height" => {
                i += 1;
                if i < args.len() {
                    opts.height = args[i].parse().unwrap_or(800);
                }
            }
            "--viewport" => opts.viewport_only = true,
            "--dump-text" => opts.dump_text = true,
            "--dump-layout" => opts.dump_layout = true,
            "--click" => {
                i += 1;
                if i < args.len() {
                    opts.click = Some(args[i].clone());
                }
            }
            other => {
                if !other.starts_with('-') {
                    opts.url = Some(other.to_string());
                }
            }
        }
        i += 1;
    }
    opts
}

fn load_for_display(url: &str, viewport: (f32, f32)) -> page::PageData {
    if url == "aurora://start" || url.is_empty() {
        return page::build_welcome_page();
    }
    match page::load_page_sized(url, None, viewport) {
        Ok(d) => d,
        Err(e) => page::build_error_page(url, &e),
    }
}

fn main() {
    let opts = parse_args();
    let url = opts.url.clone().unwrap_or_else(|| "aurora://start".to_string());

    if opts.screenshot.is_some() || opts.dump_text || opts.dump_layout {
        let viewport = (opts.width as f32, opts.height as f32);
        let data = load_for_display(&url, viewport);
        let mut fonts = FontStore::new();
        if !fonts.has_font() {
            eprintln!("Aurora: no system fonts found; text rendering will be missing.");
        }
        let t_page = std::time::Instant::now();
        let mut page = Page::new(data, &mut fonts, viewport);
        if std::env::var("AURORA_TIMING").is_ok() {
            eprintln!("[timing] page new (scripts+styles+layout): {:?}", t_page.elapsed());
        }
        // Follow script/meta-initiated navigation (a few hops).
        for _ in 0..4 {
            let nav = page
                .take_pending_navigation()
                .or_else(|| page.meta_refresh_url());
            match nav {
                Some(next) if !next.is_empty() && next != page.data.url => {
                    eprintln!("script navigation: {}", next);
                    let data = load_for_display(&next, viewport);
                    page = Page::new(data, &mut fonts, viewport);
                }
                _ => break,
            }
        }
        if let Some(selector) = &opts.click {
            if let Some(node) = page.query_selector(selector) {
                let mut target: Option<String> = None;
                {
                    let prevented = page.dispatch_js(Some(node), "click");
                    if prevented {
                        page.relayout(&mut fonts, viewport);
                    } else {
                        target = page.take_pending_navigation();
                    }
                }
                if target.is_none() {
                    let href = {
                        let doc = page.dom.borrow();
                        let mut cur = Some(node);
                        let mut found = None;
                        while let Some(n) = cur {
                            if doc.tag(n) == Some("a") && doc.attr(n, "href").is_some() {
                                found = Some(n);
                                break;
                            }
                            cur = doc.nodes[n].parent;
                        }
                        found.and_then(|n| page.link_href(n))
                    };
                    if let Some(h) = href {
                        if !h.starts_with('#') {
                            target = Some(crate::net::resolve_url(&page.data.base_url, &h));
                        }
                    }
                }
                if target.is_none() {
                    let form = {
                        let doc = page.dom.borrow();
                        let tag = doc.tag(node).unwrap_or("");
                        let ty = doc
                            .attr(node, "type")
                            .unwrap_or(if tag == "button" { "submit" } else { "text" })
                            .to_ascii_lowercase();
                        let submits = (tag == "button" || tag == "input")
                            && (ty == "submit" || ty == "image");
                        if submits {
                            page.ancestor_tag(node, "form")
                        } else {
                            None
                        }
                    };
                    if let Some(f) = form {
                        target = page.form_submit_url(f);
                    }
                }
                if let Some(url) = target {
                    if url != page.data.url && !url.starts_with('#') {
                        eprintln!("click: navigating to {}", url);
                        let data2 = load_for_display(&url, viewport);
                        page = Page::new(data2, &mut fonts, viewport);
                    }
                } else if page.take_js_changed() {
                    page.relayout(&mut fonts, viewport);
                }
            } else {
                eprintln!("click: no element matched {}", selector);
            }
        }
        if opts.dump_text {
            println!("Title: {}", page.title());
            println!("URL: {}", page.data.url);
            let doc = page.dom.borrow();
            println!("{}", doc.text_content(doc.root));
        }
        if opts.dump_layout {
            let mut out = String::new();
            let doc = page.dom.borrow();
            layout::dump_layout(&doc, &page.layout.root, 0, &mut out);
            println!("{}", out);
        }
        if let Some(out) = &opts.screenshot {
            let layout = &page.layout;
            let full_h = if opts.viewport_only {
                opts.height
            } else {
                let h = layout.page_height.max(opts.height as f32).ceil() as usize;
                h.min(20000)
            };
            let width = opts.width.max(64);
            let full_h = full_h.max(64);
            let mut pix = Pixmap::new(width, full_h, Color::WHITE);
            {
                let doc = page.dom.borrow();
                render_page(
                    &mut pix,
                    layout,
                    &doc,
                    &mut fonts,
                    &page.data.images,
                    0.0,
                    0.0,
                );
            }
            match save_png(&pix, out) {
                Ok(()) => {
                    println!(
                        "Rendered {} ({}x{}, {}x{} viewport) to {}",
                        page.data.url, width, full_h, opts.width, opts.height, out
                    );
                }
                Err(e) => {
                    eprintln!("Failed to save screenshot: {}", e);
                    std::process::exit(1);
                }
            }
        }
        return;
    }

    let initial = if opts.url.is_some() {
        Some(url)
    } else {
        None
    };
    browser::run(initial);
}
