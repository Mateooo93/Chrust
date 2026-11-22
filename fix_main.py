p = 'src/main.rs'
s = open(p, encoding='utf-8').read()
s = s.replace('''use fonts::FontStore;
use paint::{render_page, save_png, Pixmap};''', '''use fonts::FontStore;
use page::Page;
use paint::{render_page, save_png, Pixmap};''')
s = s.replace('''    if let Some(out) = &opts.screenshot {
        let data = load_for_display(&url);
        if opts.dump_text {
            println!("Title: {}", data.title);
            println!("URL: {}", data.url);
            println!("{}", data.doc.text_content(data.doc.root));
        }
        let mut fonts = FontStore::new();
        if !fonts.has_font() {
            eprintln!("Aurora: no system fonts found; text rendering will be missing.");
        }
        let viewport = (opts.width as f32, opts.height as f32);
        let (styles, layout) = data.rebuild(&mut fonts, viewport, None, None, None);
        let _ = styles;
        if opts.dump_layout {
            let mut out = String::new();
            layout::dump_layout(&data.doc, &layout.root, 0, &mut out);
            println!("{}", out);
        }
        let full_h = if opts.viewport_only {
            opts.height
        } else {
            let h = layout.page_height.max(opts.height as f32).ceil() as usize;
            h.min(20000)
        };
        let width = opts.width.max(64);
        let full_h = full_h.max(64);
        let mut pix = Pixmap::new(width, full_h, Color::WHITE);
        render_page(
            &mut pix,
            &layout,
            &data.doc,
            &mut fonts,
            &data.images,
            0.0,
            0.0,
        );
        match save_png(&pix, out) {
            Ok(()) => {
                println!(
                    "Rendered {} ({}x{}, {}x{} viewport) to {}",
                    data.url, width, full_h, opts.width, opts.height, out
                );
            }
            Err(e) => {
                eprintln!("Failed to save screenshot: {}", e);
                std::process::exit(1);
            }
        }
        return;
    }

    if opts.dump_text {
        let data = load_for_display(&url);
        println!("Title: {}", data.title);
        println!("URL: {}", data.url);
        println!("{}", data.doc.text_content(data.doc.root));
        return;
    }''', '''    if opts.screenshot.is_some() || opts.dump_text || opts.dump_layout {
        let data = load_for_display(&url);
        let mut fonts = FontStore::new();
        if !fonts.has_font() {
            eprintln!("Aurora: no system fonts found; text rendering will be missing.");
        }
        let viewport = (opts.width as f32, opts.height as f32);
        let page = Page::new(data, &mut fonts, viewport);
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
    }''')
open(p, 'w', encoding='utf-8').write(s)
print('main patched')
