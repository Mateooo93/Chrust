#![allow(dead_code)]
//! Networking: HTTP(S) with rustls, redirects, a small cookie jar, plus
//! file:// and data: URL support.

use std::collections::HashMap;
use std::io::Read;
use std::sync::{Mutex, OnceLock};
use std::time::Duration;

use url::Url;

use crate::util::{base64_decode, percent_decode};

#[derive(Clone, Debug)]
pub struct Response {
    pub url: String,
    pub status: u16,
    pub content_type: String,
    pub body: Vec<u8>,
}

impl Response {
    pub fn is_html(&self) -> bool {
        let ct = self.content_type.to_ascii_lowercase();
        ct.contains("text/html") || ct.contains("application/xhtml") || ct.is_empty()
    }
    pub fn is_css(&self) -> bool {
        self.content_type.to_ascii_lowercase().contains("text/css")
    }
}

#[derive(Clone, Debug)]
pub struct Cookie {
    name: String,
    value: String,
    domain: String,
    path: String,
}

fn cookie_jar() -> &'static Mutex<Vec<Cookie>> {
    static JAR: OnceLock<Mutex<Vec<Cookie>>> = OnceLock::new();
    JAR.get_or_init(|| Mutex::new(Vec::new()))
}

fn store_cookies(url: &Url, headers: &[String]) {
    let host = url.host_str().unwrap_or("").to_ascii_lowercase();
    let mut jar = cookie_jar().lock().unwrap();
    for h in headers {
        let mut parts = h.split(';');
        let first = match parts.next() {
            Some(f) => f.trim(),
            None => continue,
        };
        let (name, value) = match first.split_once('=') {
            Some((n, v)) => (n.trim().to_string(), v.trim().to_string()),
            None => continue,
        };
        if name.is_empty() {
            continue;
        }
        let mut domain = host.clone();
        let mut path = "/".to_string();
        for attr in parts {
            let attr = attr.trim();
            if let Some((k, v)) = attr.split_once('=') {
                let k = k.trim().to_ascii_lowercase();
                let v = v.trim();
                match k.as_str() {
                    "domain" => {
                        domain = v.trim_start_matches('.').to_ascii_lowercase();
                    }
                    "path" => path = v.to_string(),
                    _ => {}
                }
            }
        }
        if host != domain && !host.ends_with(&format!(".{}", domain)) {
            continue;
        }
        jar.retain(|c| !(c.name == name && c.domain == domain && c.path == path));
        jar.push(Cookie { name, value, domain, path });
    }
    if jar.len() > 500 {
        let excess = jar.len() - 500;
        jar.drain(0..excess);
    }
}

fn cookies_for(url: &Url) -> String {
    let host = url.host_str().unwrap_or("").to_ascii_lowercase();
    let path = url.path();
    let jar = cookie_jar().lock().unwrap();
    let mut out = Vec::new();
    for c in jar.iter() {
        if host == c.domain || host.ends_with(&format!(".{}", c.domain)) {
            if path.starts_with(&c.path) {
                out.push(format!("{}={}", c.name, c.value));
            }
        }
    }
    out.join("; ")
}

fn agent() -> ureq::Agent {
    ureq::AgentBuilder::new()
        .redirects(0)
        .timeout_connect(Duration::from_secs(6))
        .timeout_read(Duration::from_secs(12))
        .timeout_write(Duration::from_secs(12))
        .user_agent(
            "Mozilla/5.0 (Windows NT 10.0; Win64; x64) AppleWebKit/537.36 \
             (KHTML, like Gecko) Aurora/1.0 Safari/537.36",
        )
        .build()
}

pub fn fetch(url_str: &str) -> Result<Response, String> {
    let url = Url::parse(url_str).map_err(|e| format!("Invalid URL: {}", e))?;
    match url.scheme() {
        "file" => fetch_file(&url),
        "data" => fetch_data(url_str),
        "about" => Ok(Response {
            url: url_str.to_string(),
            status: 200,
            content_type: "text/html".into(),
            body: b"<!DOCTYPE html><html><head><title>About</title></head><body><h1>About</h1><p>Aurora browser.</p></body></html>".to_vec(),
        }),
        "http" | "https" => {
            let is_google = url
                .host_str()
                .map(|h| h == "google.com" || h.ends_with(".google.com"))
                .unwrap_or(false);
            let resp = fetch_http(url)?;
            if is_google {
                if let Some(fallback) = google_search_fallback(&resp) {
                    return Ok(fallback);
                }
            }
            Ok(resp)
        }
        other => Err(format!("Unsupported URL scheme: {}", other)),
    }
}

/// Google increasingly serves a JavaScript-required interstitial instead of
/// results. When that happens, transparently query DuckDuckGo's HTML endpoint
/// so a typed search still returns usable results.
fn google_search_fallback(resp: &Response) -> Option<Response> {
    let body = String::from_utf8_lossy(&resp.body[..resp.body.len().min(200_000)]);
    let interstitial = body.contains("httpservice/retry/enablejs")
        || body.contains("enablejs")
        || body.contains("Your browser isn't supported any more");
    if !interstitial {
        return None;
    }
    let parsed = Url::parse(&resp.url).ok()?;
    let is_search = parsed.path().contains("search")
        || parsed.path().contains("enablejs")
        || parsed.path().contains("httpservice");
    if !is_search {
        return None;
    }
    let q = parsed
        .query_pairs()
        .find(|(k, _)| k == "q")
        .map(|(_, v)| v.into_owned())?;
    let mut ddg = Url::parse("https://html.duckduckgo.com/html/").ok()?;
    ddg.query_pairs_mut().append_pair("q", &q);
    let fallback = match fetch_http(ddg) {
        Ok(r) => r,
        Err(_) => return None,
    };
    if fallback.status >= 400 {
        return None;
    }
    Some(fallback)
}

fn fetch_http(mut url: Url) -> Result<Response, String> {
    let agent = agent();
    for _ in 0..10 {
        let mut cookie = cookies_for(&url);
        if url.host_str().map(|h| h.ends_with("google.com")).unwrap_or(false)
            && !cookie.split(';').any(|c| c.trim_start().starts_with("SOCS="))
        {
            // Pre-accept Google's EU consent interstitial.
            let socs = "SOCS=CAESHAgBEhJnd3NfMjAyMzA4MTAtMF9SQzIaAmVuIAEaBgiA_LyaBg";
            cookie = if cookie.is_empty() {
                socs.to_string()
            } else {
                format!("{}; {}", socs, cookie)
            };
        }
        let mut req = agent
            .get(url.as_str())
            .set(
                "Accept",
                "text/html,application/xhtml+xml,application/xml;q=0.9,image/avif,image/webp,*/*;q=0.8",
            )
            .set("Accept-Language", "en-US,en;q=0.9")
            .set("Sec-CH-UA", "\"Chromium\";v=\"131\", \"Not_A Brand\";v=\"24\", \"Google Chrome\";v=\"131\"")
            .set("Sec-CH-UA-Mobile", "?0")
            .set("Sec-CH-UA-Platform", "\"Windows\"")
            .set("Sec-Fetch-Mode", "navigate")
            .set("Sec-Fetch-Dest", "document")
            .set("Upgrade-Insecure-Requests", "1");
        if !cookie.is_empty() {
            req = req.set("Cookie", &cookie);
        }
        if std::env::var("AURORA_DEBUG_HTTP").is_ok() {
            eprintln!("GET {} cookie={:?}", url.as_str(), cookie);
        }
        let resp = match req.call() {
            Ok(r) => r,
            Err(ureq::Error::Status(_, r)) => r,
            Err(e) => return Err(format!("Network error: {}", e)),
        };
        let status = resp.status();
        let set_cookies: Vec<String> = resp
            .all("Set-Cookie")
            .into_iter()
            .map(|s| s.to_string())
            .collect();
        store_cookies(&url, &set_cookies);
        if std::env::var("AURORA_DEBUG_HTTP").is_ok() {
            eprintln!("  -> {} loc={:?} setcookie={:?}", status, resp.header("Location"), set_cookies);
        }
        if (300..400).contains(&status) {
            if let Some(loc) = resp.header("Location") {
                url = url.join(loc).map_err(|e| format!("Bad redirect: {}", e))?;
                match url.scheme() {
                    "http" | "https" => continue,
                    _ => return Err("Unsupported redirect target".into()),
                }
            }
        }
        let content_type = resp.header("Content-Type").unwrap_or("").to_string();
        let length: Option<usize> = resp
            .header("Content-Length")
            .and_then(|s| s.parse().ok());
        if let Some(l) = length {
            if l > 64 * 1024 * 1024 {
                return Err("Response too large".into());
            }
        }
        let mut body = Vec::new();
        let mut reader = resp.into_reader();
        reader
            .by_ref()
            .take(64 * 1024 * 1024)
            .read_to_end(&mut body)
            .map_err(|e| format!("Read error: {}", e))?;
        return Ok(Response {
            url: url.to_string(),
            status,
            content_type,
            body,
        });
    }
    Err("Too many redirects".into())
}

fn fetch_file(url: &Url) -> Result<Response, String> {
    let path = url.to_file_path().map_err(|_| "Invalid file URL".to_string())?;
    let body = std::fs::read(&path).map_err(|e| format!("Cannot read file: {}", e))?;
    let content_type = match path
        .extension()
        .map(|e| e.to_string_lossy().to_ascii_lowercase())
        .as_deref()
    {
        Some("html") | Some("htm") => "text/html; charset=utf-8",
        Some("css") => "text/css; charset=utf-8",
        Some("js") => "text/javascript; charset=utf-8",
        Some("png") => "image/png",
        Some("jpg") | Some("jpeg") => "image/jpeg",
        Some("gif") => "image/gif",
        Some("svg") => "image/svg+xml",
        Some("webp") => "image/webp",
        Some("txt") => "text/plain; charset=utf-8",
        Some("json") => "application/json",
        _ => "application/octet-stream",
    };
    Ok(Response {
        url: url.to_string(),
        status: 200,
        content_type: content_type.to_string(),
        body,
    })
}

fn fetch_data(url_str: &str) -> Result<Response, String> {
    let rest = url_str
        .strip_prefix("data:")
        .ok_or_else(|| "Bad data URL".to_string())?;
    let (meta, payload) = rest.split_once(',').ok_or_else(|| "Bad data URL".to_string())?;
    let mut content_type = "text/plain;charset=US-ASCII".to_string();
    let mut is_base64 = false;
    for part in meta.split(';') {
        if part.eq_ignore_ascii_case("base64") {
            is_base64 = true;
        } else if part.contains('/') {
            content_type = part.to_string();
        }
    }
    let body = if is_base64 {
        base64_decode(payload).ok_or_else(|| "Invalid base64 data URL".to_string())?
    } else {
        percent_decode(payload)
    };
    Ok(Response {
        url: url_str.to_string(),
        status: 200,
        content_type,
        body,
    })
}

pub fn resolve_url(base: &str, rel: &str) -> String {
    let rel = rel.trim();
    if rel.is_empty() {
        return base.to_string();
    }
    if rel.starts_with("data:") || rel.starts_with("about:") || rel.starts_with("file:") {
        return rel.to_string();
    }
    if let Ok(b) = Url::parse(base) {
        if let Ok(u) = b.join(rel) {
            return u.to_string();
        }
    }
    rel.to_string()
}

/// Turn user input (typed URL or path) into a fetchable absolute URL.
pub fn normalize_user_url(input: &str, base: Option<&str>) -> String {
    let s = input.trim();
    if s.is_empty() {
        return "about:blank".to_string();
    }
    if s.eq_ignore_ascii_case("about:blank") || s.starts_with("about:") {
        return s.to_string();
    }
    if s.starts_with("data:") || s.starts_with("view-source:") {
        return s.to_string();
    }
    if s.starts_with("//") {
        return format!("https:{}", s);
    }
    // Windows drive paths (C:\..., C:/...) look like schemes to the check below.
    let bytes = s.as_bytes();
    if bytes.len() >= 3
        && bytes[0].is_ascii_alphabetic()
        && bytes[1] == b':'
        && (bytes[2] == 92 || bytes[2] == 47)
    {
        let path = std::path::Path::new(s);
        let abs = std::fs::canonicalize(path).unwrap_or_else(|_| path.to_path_buf());
        if let Ok(u) = Url::from_file_path(&abs) {
            return u.to_string();
        }
    }
    let has_scheme = {
        let bytes = s.as_bytes();
        let mut ok = false;
        if !bytes.is_empty() && bytes[0].is_ascii_alphabetic() {
            for (i, b) in bytes.iter().enumerate() {
                if *b == b':' {
                    ok = i > 0;
                    break;
                }
                if !(b.is_ascii_alphanumeric() || *b == b'+' || *b == b'-' || *b == b'.') {
                    break;
                }
            }
        }
        ok
    };
    if has_scheme {
        return s.to_string();
    }
    // Existing local file or path.
    let path = std::path::Path::new(s);
    if path.exists() {
        let abs = std::fs::canonicalize(path).unwrap_or_else(|_| path.to_path_buf());
        if let Ok(u) = Url::from_file_path(&abs) {
            return u.to_string();
        }
    }
    if s.starts_with('/') || s.starts_with("./") || s.starts_with("../") {
        let abs = std::path::absolute(path).unwrap_or_else(|_| path.to_path_buf());
        if let Ok(u) = Url::from_file_path(&abs) {
            return u.to_string();
        }
    }
    if let Some(b) = base {
        if !s.contains(char::is_whitespace) {
            if let Ok(u) = Url::parse(b).and_then(|bu| bu.join(s)) {
                return u.to_string();
            }
        }
    }
    if s.contains(char::is_whitespace) || !s.contains('.') {
        // Search.
        let mut enc = String::new();
        for b in s.bytes() {
            match b {
                b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'~' => enc.push(b as char),
                b' ' => enc.push('+'),
                _ => enc.push_str(&format!("%{:02X}", b)),
            }
        }
        return format!("https://www.google.com/search?q={}&hl=en&gbv=1", enc);
    }
    format!("https://{}", s)
}

pub fn decode_body(resp: &Response) -> String {
    decode_bytes(&resp.body, &resp.content_type)
}

pub fn decode_bytes(bytes: &[u8], content_type: &str) -> String {
    // BOM detection.
    if bytes.starts_with(&[0xEF, 0xBB, 0xBF]) {
        return String::from_utf8_lossy(&bytes[3..]).to_string();
    }
    if bytes.starts_with(&[0xFF, 0xFE]) {
        let mut s = String::new();
        let mut i = 2;
        while i + 1 < bytes.len() {
            let cp = u16::from_le_bytes([bytes[i], bytes[i + 1]]);
            s.push(char::from_u32(cp as u32).unwrap_or('\u{FFFD}'));
            i += 2;
        }
        return s;
    }
    let mut charset = String::new();
    let ct = content_type.to_ascii_lowercase();
    if let Some(pos) = ct.find("charset=") {
        let rest = &ct[pos + 8..];
        let end = rest
            .find(|c: char| c == ';' || c == ' ' || c == '"' || c == '\'')
            .unwrap_or(rest.len());
        charset = rest[..end].trim().to_string();
    }
    if charset.is_empty() {
        // Sniff <meta charset> from the first part of the document.
        let head = String::from_utf8_lossy(&bytes[..bytes.len().min(4096)]).to_ascii_lowercase();
        if let Some(pos) = head.find("charset=") {
            let rest = &head[pos + 8..];
            let end = rest
                .find(|c: char| c == '"' || c == '\'' || c == '>' || c == ';' || c.is_whitespace())
                .unwrap_or(rest.len());
            charset = rest[..end].to_string();
        }
    }
    if charset == "iso-8859-1" || charset == "latin1" || charset == "latin-1" {
        charset = "windows-1252".to_string();
    }
    if charset.contains("1252") || charset.contains("8859") || charset.contains("latin") {
        return decode_cp1252(bytes);
    }
    if charset.contains("utf-16") {
        let mut s = String::new();
        let mut i = 0;
        while i + 1 < bytes.len() {
            let cp = u16::from_le_bytes([bytes[i], bytes[i + 1]]);
            s.push(char::from_u32(cp as u32).unwrap_or('\u{FFFD}'));
            i += 2;
        }
        return s;
    }
    match String::from_utf8(bytes.to_vec()) {
        Ok(s) => s,
        Err(e) => {
            if charset.is_empty() {
                decode_cp1252(bytes)
            } else {
                String::from_utf8_lossy(e.as_bytes()).to_string()
            }
        }
    }
}

fn decode_cp1252(bytes: &[u8]) -> String {
    let mut s = String::with_capacity(bytes.len());
    for &b in bytes {
        if (0x80..0xA0).contains(&b) {
            let ch = match b {
                0x80 => '€', 0x82 => '‚', 0x83 => 'ƒ', 0x84 => '„', 0x85 => '…',
                0x86 => '†', 0x87 => '‡', 0x88 => 'ˆ', 0x89 => '‰', 0x8A => 'Š',
                0x8B => '‹', 0x8C => 'Œ', 0x8E => 'Ž', 0x91 => '\u{2018}',
                0x92 => '\u{2019}', 0x93 => '\u{201C}', 0x94 => '\u{201D}',
                0x95 => '•', 0x96 => '–', 0x97 => '—', 0x98 => '˜', 0x99 => '™',
                0x9A => 'š', 0x9B => '›', 0x9C => 'œ', 0x9E => 'ž', 0x9F => 'Ÿ',
                _ => '\u{FFFD}',
            };
            s.push(ch);
        } else {
            s.push(b as char);
        }
    }
    s
}

/// Load and decode an image (raster or SVG).
pub fn load_image(url: &str) -> Result<crate::layout::ImageData, String> {
    let resp = fetch(url)?;
    if resp.status >= 400 {
        return Err(format!("HTTP {}", resp.status));
    }
    let ct = resp.content_type.to_ascii_lowercase();
    let looks_svg = ct.contains("svg")
        || resp
            .body
            .iter()
            .copied()
            .find(|b| !b.is_ascii_whitespace())
            .map(|b| b == b'<')
            .unwrap_or(false)
            && String::from_utf8_lossy(&resp.body[..resp.body.len().min(400)])
                .to_ascii_lowercase()
                .contains("<svg");
    if looks_svg {
        return render_svg(&resp.body, &resp.url);
    }
    let img = image::load_from_memory(&resp.body).map_err(|e| format!("Image decode: {}", e))?;
    let rgba = img.to_rgba8();
    Ok(crate::layout::ImageData {
        width: rgba.width(),
        height: rgba.height(),
        pixels: rgba.into_raw(),
        url: resp.url,
    })
}

pub fn render_svg_markup(markup: &str, url: &str) -> Result<crate::layout::ImageData, String> {
    let opt = resvg::usvg::Options::default();
    let tree = resvg::usvg::Tree::from_str(markup, &opt)
        .map_err(|e| format!("SVG parse: {}", e))?;
    let size = tree.size();
    let mut w = size.width().ceil().max(1.0);
    let mut h = size.height().ceil().max(1.0);
    let max_dim = 1024.0f32;
    if w > max_dim || h > max_dim {
        let scale = (max_dim / w).min(max_dim / h);
        w = (w * scale).max(1.0);
        h = (h * scale).max(1.0);
    }
    let mut pixmap = resvg::tiny_skia::Pixmap::new(w as u32, h as u32)
        .ok_or_else(|| "SVG raster too large".to_string())?;
    resvg::render(
        &tree,
        resvg::tiny_skia::Transform::identity(),
        &mut pixmap.as_mut(),
    );
    let mut pixels = pixmap.take();
    for px in pixels.chunks_exact_mut(4) {
        let a = px[3] as u32;
        if a > 0 && a < 255 {
            for c in 0..3 {
                px[c] = ((px[c] as u32 * 255 + a / 2) / a).min(255) as u8;
            }
        }
    }
    Ok(crate::layout::ImageData {
        width: w as u32,
        height: h as u32,
        pixels,
        url: url.to_string(),
    })
}

fn render_svg(data: &[u8], url: &str) -> Result<crate::layout::ImageData, String> {
    let opt = resvg::usvg::Options::default();
    let tree = resvg::usvg::Tree::from_data(data, &opt)
        .map_err(|e| format!("SVG parse: {}", e))?;
    let size = tree.size();
    let mut w = size.width().ceil().max(1.0);
    let mut h = size.height().ceil().max(1.0);
    // Avoid absurdly large icon rasters.
    let max_dim = 2048.0f32;
    if w > max_dim || h > max_dim {
        let scale = (max_dim / w).min(max_dim / h);
        w = (w * scale).max(1.0);
        h = (h * scale).max(1.0);
    }
    let mut pixmap = resvg::tiny_skia::Pixmap::new(w as u32, h as u32)
        .ok_or_else(|| "SVG raster too large".to_string())?;
    resvg::render(
        &tree,
        resvg::tiny_skia::Transform::identity(),
        &mut pixmap.as_mut(),
    );
    // tiny-skia pixels are premultiplied; convert to straight alpha.
    let mut pixels = pixmap.take();
    for px in pixels.chunks_exact_mut(4) {
        let a = px[3] as u32;
        if a > 0 && a < 255 {
            for c in 0..3 {
                px[c] = ((px[c] as u32 * 255 + a / 2) / a).min(255) as u8;
            }
        }
    }
    Ok(crate::layout::ImageData {
        width: w as u32,
        height: h as u32,
        pixels,
        url: url.to_string(),
    })
}

pub struct ResourceLoader {
    pub cache: HashMap<String, Response>,
    pub fail_cache: HashMap<String, ()>,
}

impl ResourceLoader {
    pub fn new() -> ResourceLoader {
        ResourceLoader { cache: HashMap::new(), fail_cache: HashMap::new() }
    }

    pub fn fetch(&mut self, url: &str) -> Result<Response, String> {
        if let Some(r) = self.cache.get(url) {
            return Ok(r.clone());
        }
        if self.fail_cache.contains_key(url) {
            return Err("cached failure".into());
        }
        match fetch(url) {
            Ok(r) => {
                self.cache.insert(url.to_string(), r.clone());
                Ok(r)
            }
            Err(e) => {
                self.fail_cache.insert(url.to_string(), ());
                Err(e)
            }
        }
    }
}
