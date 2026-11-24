//! Site icons for tabs, bookmarks and history.
//!
//! Icons come from Google's favicon service (one PNG per site) and are cached
//! in the local data folder. Turning off "site icons" in settings
//! stops all requests.

use std::io::Read as _;
use std::path::PathBuf;
use std::sync::OnceLock;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::time::{Duration, Instant};

use gpui_kit::App;

use crate::library::{self, IconState, Shared};
use crate::storage;

/// Lowercase host of an http(s) URL, limited to characters safe in a file name.
pub fn host(url: &str) -> Option<String> {
    // Every tab and every bookmark row asks for its icon on every frame, so the
    // parse result is remembered per address.
    HOSTS.with(|hosts| {
        if let Some(known) = hosts.borrow().get(url) {
            return known.clone();
        }
        let resolved = parse_host(url);
        let mut hosts = hosts.borrow_mut();
        if hosts.len() >= 1024 {
            hosts.clear();
        }
        hosts.insert(url.to_string(), resolved.clone());
        resolved
    })
}

thread_local! {
    static HOSTS: std::cell::RefCell<std::collections::HashMap<String, Option<String>>> =
        std::cell::RefCell::new(std::collections::HashMap::new());
}

fn parse_host(url: &str) -> Option<String> {
    let parsed = url::Url::parse(url).ok()?;
    if !matches!(parsed.scheme(), "http" | "https") {
        return None;
    }
    let host = parsed.host_str()?.to_ascii_lowercase();
    let host = host.strip_prefix("www.").unwrap_or(&host).to_string();
    safe_host(&host).then_some(host)
}

/// Validates a host name coming from a URL request before it may name a
/// cache file.
pub fn safe_host(host: &str) -> bool {
    !host.is_empty()
        && host.len() <= 253
        && host.chars().all(|c| c.is_ascii_alphanumeric() || c == '.' || c == '-')
}

fn cache_path(host: &str) -> PathBuf {
    storage::favicon_dir().join(format!("{host}.png"))
}

/// The disk cache is read once per run: an icon on disk is put in `icons`
/// the first time its host is asked for, and later renders only touch it.
static SCANNED: OnceLock<()> = OnceLock::new();

fn scan_cache(lib: &Shared) {
    SCANNED.get_or_init(|| {
        let Ok(entries) = std::fs::read_dir(storage::favicon_dir()) else { return };
        let mut found = Vec::new();
        for entry in entries.flatten() {
            let path = entry.path();
            if path.extension().is_none_or(|e| e != "png") {
                continue;
            }
            if let Some(host) = path.file_stem().and_then(|s| s.to_str())
                && std::fs::metadata(&path).is_ok_and(|m| m.len() > 0)
            {
                found.push((host.to_string(), path));
            }
        }
        if found.is_empty() {
            return;
        }
        // Outside an update nothing else can be borrowing it; a miss here
        // only means a few icons are looked up on disk again.
        if let Ok(mut lib) = lib.try_borrow_mut() {
            for (host, path) in found {
                lib.icons.entry(host).or_insert(IconState::Ready(path));
            }
        }
    });
}

/// How long a download may run before another attempt is allowed. Long enough
/// for a slow site on a bad connection, short enough that a dropped thread does
/// not leave the icon missing for the session.
const PENDING_TTL: Duration = Duration::from_secs(20);

/// At most this many favicon downloads run at once; the rest wait their turn
/// instead of competing for bandwidth and connections.
const MAX_DOWNLOADS: usize = 4;
static DOWNLOADS: AtomicUsize = AtomicUsize::new(0);

fn downloading() -> bool {
    DOWNLOADS.load(Ordering::Relaxed) >= MAX_DOWNLOADS
}

/// The cached icon for `url`, if there is one. Starts a download the first
/// time a site is seen and refreshes the windows when it arrives.
pub fn icon(url: &str, cx: &mut App) -> Option<PathBuf> {
    let lib = library::get(cx);
    if !lib.borrow().settings.site_icons || crate::address::is_internal(url) {
        return None;
    }
    let host = host(url)?;
    scan_cache(&lib);
    {
        let mut lib = lib.borrow_mut();
        match lib.icons.get(&host) {
            Some(IconState::Ready(path)) => return Some(path.clone()),
            // A remembered failure is retried after a while, not per render.
            Some(IconState::Missing(since)) if since.elapsed() < library::MISSING_ICON_TTL => return None,
            // A download is already on its way; asking again would start a
            // second one for the same site on the next frame.
            Some(IconState::Pending(since)) if since.elapsed() < PENDING_TTL => return None,
            Some(IconState::Missing(_) | IconState::Pending(_)) | None => {}
        }
        // Too many at once: leave it unasked, so the next frame tries again
        // instead of competing for connections.
        if downloading() {
            return None;
        }
        let path = cache_path(&host);
        if std::fs::metadata(&path).is_ok_and(|m| m.len() > 0) {
            lib.icons.insert(host, IconState::Ready(path.clone()));
            return Some(path);
        }
        lib.icons.insert(host.clone(), IconState::Pending(Instant::now()));
    }

    // A plain thread, so slow sites never hold up GPUI's executor threads.
    let (tx, rx) = futures::channel::oneshot::channel();
    let fetch_host = host.clone();
    DOWNLOADS.fetch_add(1, Ordering::Relaxed);
    std::thread::spawn(move || {
        let _ = tx.send(download(&fetch_host));
    });
    cx.spawn(async move |cx| {
        let result = rx.await.ok().flatten();
        DOWNLOADS.fetch_sub(1, Ordering::Relaxed);
        let _ = cx.update(|cx| {
            let lib = library::get(cx);
            let state = match result {
                Some(path) => IconState::Ready(path),
                None => IconState::Missing(Instant::now()),
            };
            lib.borrow_mut().icons.insert(host, state);
            cx.refresh_windows();
        });
    })
    .detach();
    None
}

fn download(host: &str) -> Option<PathBuf> {
    // One agent for the whole process, so DNS, TLS and connections are
    // shared between icon downloads instead of rebuilt for each site.
    static AGENT: OnceLock<ureq::Agent> = OnceLock::new();
    let agent = AGENT.get_or_init(|| ureq::AgentBuilder::new().timeout(Duration::from_secs(8)).build());
    let url = format!("https://www.google.com/s2/favicons?domain={host}&sz=64");
    let response = agent.get(&url).call().ok()?;
    if !response.content_type().starts_with("image/") {
        return None;
    }
    let mut bytes = Vec::new();
    response.into_reader().take(512 * 1024).read_to_end(&mut bytes).ok()?;
    if bytes.is_empty() {
        return None;
    }
    let path = cache_path(host);
    std::fs::create_dir_all(path.parent()?).ok()?;
    std::fs::write(&path, &bytes).ok()?;
    Some(path)
}

#[cfg(test)]
mod tests {
    use super::host;

    #[test]
    fn hosts() {
        assert_eq!(host("https://www.YouTube.com/watch?v=1").as_deref(), Some("youtube.com"));
        assert_eq!(host("http://localhost:3000/").as_deref(), Some("localhost"));
        assert_eq!(host("file:///C:/x.html"), None);
        assert_eq!(host("about:blank"), None);
    }
}
