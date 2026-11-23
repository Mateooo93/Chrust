#![allow(dead_code)]
//! Page loading and the live page model.

use std::cell::RefCell;
use std::collections::{HashMap, HashSet};
use std::rc::Rc;

use crate::css::{extract_url, parse_stylesheet, StyleSheet};
use crate::dom::Document;
use crate::js::JsRuntime;
use crate::fonts::FontStore;
use crate::html::parse_document;
use crate::layout::{ImageData, LayoutResult};
use crate::net::{decode_body, decode_bytes, fetch, normalize_user_url, resolve_url, ResourceLoader, Response};
use crate::style::{compute_styles, ComputedStyle};

pub struct ScriptItem {
    pub text: String,
    pub url: Option<String>,
}

pub struct PageData {
    pub doc: Document,
    pub stylesheet: StyleSheet,
    pub images: HashMap<String, ImageData>,
    pub base_url: String,
    pub url: String,
    pub title: String,
    pub scripts: Vec<ScriptItem>,
    pub status: u16,
}

impl PageData {
    pub fn rebuild(&self, fonts: &mut FontStore, viewport: (f32, f32), hovered: Option<usize>, active: Option<usize>, focused: Option<usize>) -> (Vec<ComputedStyle>, LayoutResult) {
        let styles = compute_styles(&self.doc, &self.stylesheet, viewport, hovered, active, focused);
        let mut engine = crate::layout::LayoutEngine::new(
            &self.doc,
            &styles,
            fonts,
            &self.images,
            viewport,
            &self.base_url,
        );
        let layout = engine.layout_document();
        (styles, layout)
    }

    pub fn link_href(&self, node: usize) -> Option<String> {
        let href = self.doc.attr(node, "href")?;
        let href = href.trim();
        if href.starts_with('#') {
            return Some(href.to_string());
        }
        if href.is_empty() {
            return None;
        }
        Some(resolve_url(&self.base_url, href))
    }
}

pub struct Page {
    pub data: PageData,
    pub dom: Rc<RefCell<Document>>,
    pub js: Option<JsRuntime>,
    pub styles: Vec<ComputedStyle>,
    pub layout: LayoutResult,
    pub viewport: (f32, f32),
    pub hovered: Option<usize>,
    pub focused: Option<usize>,
}

impl Page {
    pub fn new(mut data: PageData, fonts: &mut FontStore, viewport: (f32, f32)) -> Page {
        let doc = std::mem::replace(&mut data.doc, Document::new());
        let dom = Rc::new(RefCell::new(doc));
        let mut js = JsRuntime::new(
            dom.clone(),
            &data.base_url,
            (viewport.0 as f64, viewport.1 as f64),
        );
        js.run_scripts(&data.scripts);
        let title = js.shared.dom.borrow().title();
        if !title.is_empty() {
            data.title = title;
        }
        let (styles, layout) = {
            let doc = dom.borrow();
            let styles = compute_styles(
                &doc,
                &data.stylesheet,
                viewport,
                None,
                None,
                None,
            );
            let layout = {
                let mut engine = crate::layout::LayoutEngine::new(
                    &doc,
                    &styles,
                    fonts,
                    &data.images,
                    viewport,
                    &data.base_url,
                );
                engine.layout_document()
            };
            (styles, layout)
        };
        Page {
            data,
            dom,
            js: Some(js),
            styles,
            layout,
            viewport,
            hovered: None,
            focused: None,
        }
    }

    pub fn query_selector(&self, selector: &str) -> Option<usize> {
        let doc = self.dom.borrow();
        let mut sels = Vec::new();
        for part in crate::css::split_top_level(selector, ',') {
            if let Some(sel) = crate::css::parse_selector(&part) {
                sels.push(sel);
            }
        }
        if sels.is_empty() {
            return None;
        }
        let ctx = crate::css::MatchContext {
            doc: &doc,
            hovered: None,
            active: None,
            focused: None,
        };
        doc.walk(doc.root)
            .into_iter()
            .find(|&n| doc.is_element(n) && sels.iter().any(|s| s.matches(n, &ctx)))
    }

    pub fn title(&self) -> String {
        let t = self.dom.borrow().title();
        if t.is_empty() {
            self.data.title.clone()
        } else {
            t
        }
    }

    pub fn ancestor_tag(&self, node: usize, tag: &str) -> Option<usize> {
        let doc = self.dom.borrow();
        let mut cur = Some(node);
        while let Some(n) = cur {
            if doc.tag(n) == Some(tag) {
                return Some(n);
            }
            cur = doc.nodes[n].parent;
        }
        None
    }

    pub fn meta_refresh_url(&self) -> Option<String> {
        let doc = self.dom.borrow();
        for n in doc.walk(doc.root) {
            if doc.tag(n) != Some("meta") {
                continue;
            }
            let equiv = doc.attr(n, "http-equiv").unwrap_or("").to_ascii_lowercase();
            if equiv != "refresh" {
                continue;
            }
            let content = doc.attr(n, "content")?;
            let (delay, target) = match content.split_once(';') {
                Some((d, t)) => (d.trim().parse::<f32>().unwrap_or(0.0), t.trim()),
                None => (content.trim().parse::<f32>().unwrap_or(0.0), ""),
            };
            if delay > 10.0 {
                continue;
            }
            let target = target.trim();
            let target = match target.to_ascii_lowercase().strip_prefix("url=") {
                Some(_) => &target[4..],
                None => target,
            };
            let target = target.trim().trim_matches(|c| c == '\'' || c == '"');
            if target.is_empty() {
                continue;
            }
            return Some(resolve_url(&self.data.base_url, target));
        }
        None
    }

    pub fn take_pending_navigation(&mut self) -> Option<String> {
        self.js.as_mut().and_then(|js| js.take_pending_navigation())
    }

    pub fn has_js_timers(&self) -> bool {
        self.js
            .as_ref()
            .map(|js| !js.shared.timers.borrow().is_empty())
            .unwrap_or(false)
    }

    pub fn js_tick(&mut self, fonts: &mut FontStore) -> bool {
        if let Some(js) = &mut self.js {
            js.tick_timers();
        }
        let changed = self.take_js_changed();
        if changed {
            let vp = self.viewport;
            self.relayout(fonts, vp);
        }
        changed
    }

    pub fn form_submit_url(&self, form: usize) -> Option<String> {
        let doc = self.dom.borrow();
        let action = doc.attr(form, "action").unwrap_or("").to_string();
        let action_url = if action.is_empty() {
            self.data.url.clone()
        } else {
            resolve_url(&self.data.base_url, action.trim())
        };
        let method = doc
            .attr(form, "method")
            .unwrap_or("get")
            .to_ascii_lowercase();
        let mut pairs: Vec<(String, String)> = Vec::new();
        let mut push = |name: String, value: String| {
            if !name.is_empty() {
                pairs.push((name, value));
            }
        };
        for n in doc.walk(form) {
            match doc.tag(n) {
                Some("input") => {
                    if doc.attr(n, "disabled").is_some() {
                        continue;
                    }
                    let name = doc.attr(n, "name").unwrap_or("").to_string();
                    if name.is_empty() {
                        continue;
                    }
                    let ty = doc.attr(n, "type").unwrap_or("text").to_ascii_lowercase();
                    if ty == "submit" || ty == "button" || ty == "reset" || ty == "file" || ty == "image" {
                        continue;
                    }
                    if (ty == "checkbox" || ty == "radio") && doc.attr(n, "checked").is_none() {
                        continue;
                    }
                    let value = doc.attr(n, "value").unwrap_or("").to_string();
                    push(name, value);
                }
                Some("textarea") => {
                    let name = doc.attr(n, "name").unwrap_or("").to_string();
                    let value = doc.text_content(n);
                    push(name, value);
                }
                Some("select") => {
                    let name = doc.attr(n, "name").unwrap_or("").to_string();
                    if name.is_empty() {
                        continue;
                    }
                    let mut value = String::new();
                    for c in &doc.nodes[n].children {
                        if doc.tag(*c) == Some("option") {
                            if doc.attr(*c, "selected").is_some() || value.is_empty() {
                                let t = doc.text_content(*c).trim().to_string();
                                value = doc.attr(*c, "value").unwrap_or(&t).to_string();
                                if doc.attr(*c, "selected").is_some() {
                                    break;
                                }
                            }
                        }
                    }
                    push(name, value);
                }
                _ => {}
            }
        }
        let query = pairs
            .iter()
            .map(|(k, v)| {
                format!(
                    "{}={}",
                    crate::util::url_encode_component(k),
                    crate::util::url_encode_component(v)
                )
            })
            .collect::<Vec<_>>()
            .join("&");
        let _ = method;
        if query.is_empty() {
            return Some(action_url);
        }
        if action_url.contains('?') {
            Some(format!("{}&{}", action_url, query))
        } else {
            Some(format!("{}?{}", action_url, query))
        }
    }

    pub fn link_href(&self, node: usize) -> Option<String> {
        let doc = self.dom.borrow();
        let href = doc.attr(node, "href")?;
        let href = href.trim();
        if href.starts_with('#') {
            return Some(href.to_string());
        }
        if href.is_empty() {
            return None;
        }
        Some(resolve_url(&self.data.base_url, href))
    }

    pub fn dispatch_js(&mut self, node: Option<usize>, event: &str) -> bool {
        if let Some(js) = &mut self.js {
            js.dispatch(node, event)
        } else {
            false
        }
    }

    pub fn take_js_changed(&mut self) -> bool {
        if let Some(js) = &mut self.js {
            js.take_changed()
        } else {
            false
        }
    }

    pub fn relayout(&mut self, fonts: &mut FontStore, viewport: (f32, f32)) {
        if viewport.0 < 1.0 || viewport.1 < 1.0 {
            return;
        }
        self.viewport = viewport;
        let (styles, layout) = {
            let doc = self.dom.borrow();
            let styles = compute_styles(
                &doc,
                &self.data.stylesheet,
                viewport,
                self.hovered,
                None,
                self.focused,
            );
            let layout = {
                let mut engine = crate::layout::LayoutEngine::new(
                    &doc,
                    &styles,
                    fonts,
                    &self.data.images,
                    viewport,
                    &self.data.base_url,
                );
                engine.layout_document()
            };
            (styles, layout)
        };
        self.styles = styles;
        self.layout = layout;
    }

    pub fn set_hover(&mut self, fonts: &mut FontStore, node: Option<usize>) {
        if node == self.hovered {
            return;
        }
        self.hovered = node;
        self.relayout(fonts, self.viewport);
    }

    pub fn set_focused(&mut self, fonts: &mut FontStore, node: Option<usize>) {
        if node == self.focused {
            return;
        }
        self.focused = node;
        self.relayout(fonts, self.viewport);
    }

    pub fn scroll_height(&self) -> f32 {
        self.layout.page_height.max(self.viewport.1)
    }
}

pub fn build_error_page(url: &str, message: &str) -> PageData {
    let html = format!(
        "<!DOCTYPE html><html><head><title>Problem loading page</title></head>\
         <body style=\"font-family: 'Segoe UI', sans-serif; background: #f9f9fa;\">\
         <div style=\"max-width: 640px; margin: 60px auto; background: white; border: 1px solid #ddd; border-radius: 8px; padding: 28px 32px;\">\
         <h1 style=\"font-size: 24px; margin-top: 0;\">This page could not be loaded</h1>\
         <p style=\"color: #555;\">Aurora could not load <b>{}</b>.</p>\
         <pre style=\"white-space: pre-wrap; background: #f1f1f3; padding: 12px; border-radius: 4px; color: #a00;\">{}</pre>\
         </div></body></html>",
        crate::util::escape_html(url),
        crate::util::escape_html(message)
    );
    let t_parse = std::time::Instant::now();
    let doc = parse_document(&html);
    timing_log("html parse", t_parse);
    let title = doc.title();
    let stylesheet = inline_stylesheet(&doc);
    PageData {
        doc,
        stylesheet,
        images: HashMap::new(),
        base_url: url.to_string(),
        url: url.to_string(),
        title,
        scripts: Vec::new(),
        status: 0,
    }
}

fn collect_stylesheets(
    doc: &Document,
    base: &str,
    loader: &mut ResourceLoader,
    styles: &mut String,
    viewport: (f32, f32),
) {
    for node in doc.walk(doc.root) {
        match doc.tag(node) {
            Some("style") => {
                if let Some(media) = doc.attr(node, "media") {
                    if !crate::css::stylesheet_media_ok(media, viewport) {
                        continue;
                    }
                }
                let text = doc.text_content(node);
                if doc.attr(node, "type").map(|t| t.to_ascii_lowercase().contains("css")).unwrap_or(true) {
                    styles.push_str(&text);
                    styles.push('\n');
                }
            }
            Some("link") => {
                let rel = doc.attr(node, "rel").unwrap_or("").to_ascii_lowercase();
                if !rel.split_whitespace().any(|r| r == "stylesheet") {
                    continue;
                }
                if rel.contains("alternate") {
                    continue;
                }
                if let Some(media) = doc.attr(node, "media") {
                    if !crate::css::stylesheet_media_ok(media, viewport) {
                        continue;
                    }
                }
                if let Some(href) = doc.attr(node, "href") {
                    let abs = resolve_url(base, href);
                    if let Ok(resp) = loader.fetch(&abs) {
                        if resp.status < 400 {
                            let css = decode_body(&resp);
                            let mut imported = String::new();
                            load_imports(&css, &resp.url, loader, &mut imported, 0);
                            styles.push_str(&imported);
                            styles.push_str(&css);
                            styles.push('\n');
                        }
                    }
                }
            }
            _ => {}
        }
    }
}

fn load_imports(css: &str, base: &str, loader: &mut ResourceLoader, out: &mut String, depth: u32) {
    if depth > 4 || !css.contains("@import") {
        return;
    }
    let mut pos = 0;
    while let Some(idx) = css[pos..].find("@import") {
        let start = pos + idx + 7;
        let rest = &css[start..];
        let end = rest.find(';').unwrap_or(rest.len());
        let stmt = rest[..end].trim();
        let mut url = None;
        if let Some(u) = stmt.strip_prefix("url(") {
            let inner = u.trim_end_matches(')').trim();
            url = Some(inner.trim_matches('"').trim_matches('\'').to_string());
        } else if let Some(q) = stmt.strip_prefix('"').or_else(|| stmt.strip_prefix('\'')) {
            if let Some(e) = q.find(['"', '\'']) {
                url = Some(q[..e].to_string());
            }
        }
        if let Some(u) = url {
            let abs = resolve_url(base, &u);
            if let Ok(resp) = loader.fetch(&abs) {
                let text = decode_body(&resp);
                load_imports(&text, &resp.url, loader, out, depth + 1);
                out.push_str(&text);
                out.push('\n');
            }
        }
        pos = start + end + 1;
        if pos >= css.len() {
            break;
        }
    }
}

fn collect_urls_from_css(css: &StyleSheet) -> Vec<String> {
    let mut urls = Vec::new();
    for rule in &css.rules {
        for d in &rule.decls {
            if d.value.contains("url(") {
                if let Some(u) = extract_url(&d.value) {
                    urls.push(u);
                }
            }
        }
    }
    urls
}

fn collect_scripts(doc: &Document, base: &str, loader: &mut ResourceLoader) -> Vec<ScriptItem> {
    let mut scripts = Vec::new();
    for node in doc.walk(doc.root) {
        if doc.tag(node) != Some("script") {
            continue;
        }
        let ty = doc.attr(node, "type").unwrap_or("").to_ascii_lowercase();
        // ES modules need import/export support we don't provide yet; treating
        // them as classic scripts only produces syntax errors and wasted work.
        if ty.contains("module") {
            continue;
        }
        if !ty.is_empty() && !ty.contains("javascript") && !ty.contains("ecmascript") {
            continue;
        }
        if let Some(src) = doc.attr(node, "src") {
            let abs = resolve_url(base, src);
            if let Ok(resp) = loader.fetch(&abs) {
                if resp.status < 400 {
                    let text = decode_body(&resp);
                    scripts.push(ScriptItem { text, url: Some(abs) });
                }
            }
        } else {
            let text = doc.text_content(node);
            if !text.trim().is_empty() {
                scripts.push(ScriptItem { text, url: None });
            }
        }
    }
    scripts
}

pub fn load_page(url_input: &str, current_url: Option<&str>) -> Result<PageData, String> {
    load_page_sized(url_input, current_url, (1280.0, 800.0))
}

pub fn load_page_sized(
    url_input: &str,
    current_url: Option<&str>,
    viewport: (f32, f32),
) -> Result<PageData, String> {
    let url = normalize_user_url(url_input, current_url);
    let resp = fetch(&url)?;
    page_from_response(resp, &url, viewport)
}

fn timing_log(label: &str, start: std::time::Instant) {
    if std::env::var("AURORA_TIMING").is_ok() {
        eprintln!("[timing] {}: {:?}", label, start.elapsed());
    }
}

fn page_from_response(
    resp: Response,
    requested_url: &str,
    viewport: (f32, f32),
) -> Result<PageData, String> {
    let final_url = resp.url.clone();
    let ct = resp.content_type.to_ascii_lowercase();
    if resp.status >= 400 && resp.body.is_empty() {
        return Err(format!("HTTP error {}", resp.status));
    }
    if ct.starts_with("image/") {
        let img = image::load_from_memory(&resp.body).map_err(|e| e.to_string())?;
        let rgba = img.to_rgba8();
        let (w, h) = (rgba.width(), rgba.height());
        let html = format!(
            "<!DOCTYPE html><html><head><title>{}</title></head><body style=\"margin:0;background:#222;text-align:center\"><img src=\"{}\" style=\"max-width:100%\"></body></html>",
            final_url, final_url
        );
        let doc = parse_document(&html);
        let mut images = HashMap::new();
        images.insert(
            final_url.clone(),
            ImageData { width: w, height: h, pixels: rgba.into_raw(), url: final_url.clone() },
        );
        return Ok(PageData {
            doc,
            stylesheet: StyleSheet::default(),
            images,
            base_url: final_url.clone(),
            url: final_url,
            title: requested_url.to_string(),
            scripts: Vec::new(),
            status: resp.status,
        });
    }
    if ct.starts_with("text/plain") {
        let text = decode_body(&resp);
        let html = format!(
            "<!DOCTYPE html><html><head><title>{}</title></head><body><pre style=\"white-space:pre-wrap;font-family:Consolas,monospace\">{}</pre></body></html>",
            crate::util::escape_html(&final_url),
            crate::util::escape_html(&text)
        );
        let doc = parse_document(&html);
        return Ok(PageData {
            doc,
            stylesheet: StyleSheet::default(),
            images: HashMap::new(),
            base_url: final_url.clone(),
            url: final_url,
            title: requested_url.to_string(),
            scripts: Vec::new(),
            status: resp.status,
        });
    }
    let text = decode_bytes(&resp.body, &resp.content_type);
    let doc = parse_document(&text);
    let title = {
        let t = doc.title();
        if t.is_empty() {
            final_url.clone()
        } else {
            t
        }
    };
    // Base URL.
    let mut base = final_url.clone();
    for node in doc.walk(doc.root) {
        if doc.tag(node) == Some("base") {
            if let Some(href) = doc.attr(node, "href") {
                base = resolve_url(&final_url, href);
                break;
            }
        }
    }
    let mut loader = ResourceLoader::new();
    // Cache the main document so relative fetches that hit it are cheap.
    loader.cache.insert(
        final_url.clone(),
        Response {
            url: final_url.clone(),
            status: resp.status,
            content_type: resp.content_type.clone(),
            body: resp.body.clone(),
        },
    );
    let mut css_text = String::new();
    let t_cssfetch = std::time::Instant::now();
    collect_stylesheets(&doc, &base, &mut loader, &mut css_text, viewport);
    timing_log("css fetch", t_cssfetch);
    timing_log("css bytes", std::time::Instant::now());
    if let Ok(path) = std::env::var("AURORA_CSS_DUMP") {
        let _ = std::fs::write(path, &css_text);
    }
    let t_css = std::time::Instant::now();
    let stylesheet = parse_stylesheet(&css_text);
    timing_log("css parse", t_css);
    let t_scripts = std::time::Instant::now();
    let scripts = collect_scripts(&doc, &base, &mut loader);
    timing_log("script fetch", t_scripts);

    // Collect image URLs.
    let mut urls: Vec<String> = Vec::new();
    let mut seen: HashSet<String> = HashSet::new();
    for node in doc.walk(doc.root) {
        if doc.tag(node) != Some("img") {
            continue;
        }
        let src = doc
            .attr(node, "src")
            .or_else(|| doc.attr(node, "data-src"))
            .or_else(|| doc.attr(node, "data-original"))
            .filter(|s| !s.trim().is_empty())
            .map(|s| s.to_string())
            .or_else(|| {
                doc.attr(node, "srcset")
                    .or_else(|| doc.attr(node, "data-srcset"))
                    .and_then(crate::util::srcset_first)
            });
        if let Some(src) = src {
            let abs = resolve_url(&base, src.trim());
            if !abs.is_empty() && seen.insert(abs.clone()) {
                urls.push(abs);
            }
        }
    }
    for u in collect_urls_from_css(&stylesheet) {
        let abs = resolve_url(&base, &u);
        if seen.insert(abs.clone()) {
            urls.push(abs);
        }
    }
    // Styles may contain background images in inline styles too.
    for node in doc.walk(doc.root) {
        if let Some(style) = doc.attr(node, "style") {
            if style.contains("url(") {
                if let Some(u) = extract_url(style) {
                    let abs = resolve_url(&base, &u);
                    if seen.insert(abs.clone()) {
                        urls.push(abs);
                    }
                }
            }
        }
    }
    if std::env::var("AURORA_DEBUG_IMG").is_ok() {
        eprintln!("image urls collected: {}", urls.len());
    }
    let urls_total = urls.len();
    let urls: Vec<String> = urls.into_iter().take(140).collect();
    let results: std::sync::Mutex<Vec<(String, ImageData)>> =
        std::sync::Mutex::new(Vec::new());
    let next = std::sync::atomic::AtomicUsize::new(0);
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(8);
    std::thread::scope(|scope| {
        for _ in 0..8 {
            scope.spawn(|| loop {
                if std::time::Instant::now() >= deadline {
                    break;
                }
                let i = next.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
                if i >= urls.len() {
                    break;
                }
                let u = &urls[i];
                if u.starts_with("data:") {
                    if let Ok(resp) = fetch(u) {
                        if let Ok(img) = image::load_from_memory(&resp.body) {
                            let rgba = img.to_rgba8();
                            results.lock().unwrap().push((
                                u.clone(),
                                ImageData {
                                    width: rgba.width(),
                                    height: rgba.height(),
                                    pixels: rgba.into_raw(),
                                    url: u.clone(),
                                },
                            ));
                        }
                    }
                    continue;
                }
                match crate::net::load_image(u) {
                    Ok(img) => {
                        results.lock().unwrap().push((u.clone(), img));
                    }
                    Err(e) => {
                        if std::env::var("AURORA_DEBUG_IMG").is_ok() {
                            eprintln!("image failed: {} ({})", u, e);
                        }
                    }
                }
            });
        }
    });
    let t_images = std::time::Instant::now();
    let mut images = HashMap::new();
    for (k, v) in results.into_inner().unwrap() {
        images.insert(k, v);
    }
    timing_log("images", t_images);
    if std::env::var("AURORA_DEBUG_IMG").is_ok() {
        eprintln!("images loaded: {} / attempts {}", images.len(), urls_total);
    }
    Ok(PageData {
        doc,
        stylesheet,
        images,
        base_url: base,
        url: final_url,
        title,
        scripts,
        status: resp.status,
    })
}

fn inline_stylesheet(doc: &Document) -> StyleSheet {
    let mut css = String::new();
    for n in doc.walk(doc.root) {
        if doc.tag(n) == Some("style") {
            css.push_str(&doc.text_content(n));
            css.push('\n');
        }
    }
    parse_stylesheet(&css)
}

pub fn build_welcome_page() -> PageData {
    let html = r#"<!DOCTYPE html>
<html><head><title>New Tab</title>
<style>
  html, body { height: 100%; }
  body { margin: 0; background-color: #eef1f7; color: #1c2430; font-family: 'Segoe UI', sans-serif; }
  .wrap { max-width: 860px; margin: 0 auto; padding: 68px 32px 48px; }
  .brand { display: flex; align-items: center; gap: 14px; margin-bottom: 8px; }
  .mark { width: 46px; height: 46px; border-radius: 13px; background-color: #3555d4; color: white; text-align: center; font-size: 30px; font-weight: bold; line-height: 46px; }
  h1 { font-size: 38px; margin: 0; letter-spacing: -0.5px; }
  .sub { color: #5a6575; font-size: 17px; margin: 8px 0 36px; }
  .search { display: flex; background: white; border: 1px solid #cfd6e2; border-radius: 12px; padding: 6px 8px; box-shadow: 0 6px 22px rgba(30,50,90,0.08); }
  .search .icon { font-size: 17px; color: #7a8496; padding: 8px 10px; }
  .search .hint { flex: 1; color: #8a93a5; padding: 9px 2px; font-size: 15px; }
  .cards { display: flex; gap: 16px; margin-top: 34px; flex-wrap: wrap; }
  .card { flex: 1; min-width: 170px; background: white; border: 1px solid #d8dee9; border-radius: 12px; padding: 18px 20px; text-decoration: none; color: #1c2430; }
  .card b { display: block; font-size: 15px; margin-bottom: 4px; color: #223; }
  .card span { color: #6b7587; font-size: 13px; }
  .foot { margin-top: 44px; color: #8590a2; font-size: 12.5px; line-height: 1.6; border-top: 1px solid #dbe1ea; padding-top: 18px; }
  .pill { display: inline-block; background: #dfe6f7; color: #2f4c9e; border-radius: 20px; font-size: 12px; padding: 3px 10px; margin-left: 10px; vertical-align: middle; }
</style></head>
<body><div class="wrap">
  <div class="brand"><div class="mark">A</div><h1>Aurora<span class="pill">native Rust engine</span></h1></div>
  <p class="sub">A browser built from scratch in Rust &mdash; HTML parser, CSS cascade, layout, paint, and networking.</p>
  <div class="search"><div class="icon">&#128269;</div><div class="hint">Type a URL in the address bar above to explore the web</div></div>
  <div class="cards">
    <a class="card" href="https://example.com"><b>example.com</b><span>A tiny HTML test page</span></a>
    <a class="card" href="https://en.wikipedia.org/wiki/Web_browser"><b>Wikipedia</b><span>Web browser article</span></a>
    <a class="card" href="https://news.ycombinator.com"><b>Hacker News</b><span>Links and discussion</span></a>
  </div>
  <div class="foot">
    Ctrl+L &nbsp;&middot;&nbsp; focus address bar &nbsp;&nbsp; Alt+Left / Alt+Right &nbsp;&middot;&nbsp; history &nbsp;&nbsp; Ctrl+R &nbsp;&middot;&nbsp; reload<br>
    HTML5 tokenizer &middot; CSS cascade &middot; block/inline layout &middot; flexbox &middot; tables &middot; software rasterizer &middot; HTTP via rustls
  </div>
</div></body></html>"#;
    let doc = parse_document(html);
    let title = doc.title();
    let stylesheet = inline_stylesheet(&doc);
    PageData {
        doc,
        stylesheet,
        images: HashMap::new(),
        base_url: "aurora://start".to_string(),
        url: "aurora://start".to_string(),
        title,
        scripts: Vec::new(),
        status: 200,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn welcome_has_styles() {
        let p = build_welcome_page();
        println!("rules={} title={:?} text={:?}", p.stylesheet.rules.len(), p.title, &p.doc.text_content(p.doc.root)[..80.min(p.doc.text_content(p.doc.root).len())]);
        for r in &p.stylesheet.rules {
            println!("  sel {:?} decls {}", r.selectors.first().map(|s| s.compounds.len()), r.decls.len());
        }
        assert!(!p.stylesheet.rules.is_empty());
    }
}

#[cfg(test)]
mod layout_tests {
    use super::*;
    #[test]
    fn welcome_layout_debug() {
        let p = build_welcome_page();
        let mut fonts = crate::fonts::FontStore::new();
        let (styles, layout) = p.rebuild(&mut fonts, (1200.0, 800.0), None, None, None);
        for n in p.doc.walk(p.doc.root) {
            if let Some(cls) = p.doc.attr(n, "class") {
                let tag = p.doc.tag(n).unwrap_or("");
                println!("{} .{} margin={:?} display={:?} width={:?}", tag, cls, styles[n].margin, styles[n].display, styles[n].width);
            }
        }
        println!("page_height={} root_rect={:?} body_rect={:?}", layout.page_height, layout.root.rect,
            p.doc.walk(p.doc.root).into_iter().find(|&n| p.doc.tag(n)==Some("body")).map(|b| crate::layout::hit_test(&layout.root, 1.0, 1.0)).map(|_| ()));
    }
}

#[cfg(test)]
mod layout_debug2 {
    use super::*;
    fn dump(doc: &crate::dom::Document, b: &crate::layout::LayoutBox, depth: usize) {
        let tag = doc.tag(b.node).unwrap_or("text");
        let cls = doc.attr(b.node, "class").unwrap_or("");
        println!("{}{} .{} rect=({:.0},{:.0},{:.0},{:.0}) content=({:.0},{:.0},{:.0},{:.0}) lines={}", "  ".repeat(depth), tag, cls, b.rect.x, b.rect.y, b.rect.w, b.rect.h, b.content_rect.x, b.content_rect.y, b.content_rect.w, b.content_rect.h, b.lines.len());
        for c in &b.children { dump(doc, c, depth+1); }
    }
    #[test]
    fn dump_boxes() {
        let p = build_welcome_page();
        let mut fonts = crate::fonts::FontStore::new();
        let (_, layout) = p.rebuild(&mut fonts, (1200.0, 800.0), None, None, None);
        dump(&p.doc, &layout.root, 0);
    }
}
