//! Ad and tracker blocking.
//!
//! Rules are compiled once per process by Brave's `adblock` crate, the engine
//! behind Brave's own shields, so the same Adblock Plus syntax works here as
//! in uBlock Origin. Two lists ship inside the executable (see
//! `assets/filters`): a host blocklist and a short curated list that also
//! covers YouTube's in-stream ads, which live on Google's own hosts and on
//! youtube.com itself. When `adblock_upstream` is on, EasyList and EasyPrivacy
//! are fetched as well and rebuild the engine, with the bundled lists kept as
//! a base.
//!
//! The engine is rebuilt by writing a whole new value behind an `RwLock`, so a
//! page asking "is this URL an ad?" never waits on a download.
//!
//! Blocking itself happens in `native` (WebView2 resource interception); this
//! module only answers the question.
//!
//! Cosmetic rules (`##selector`, scriptlets) are compiled too, but WebView2
//! cannot inject a stylesheet the way a browser with its own content scripts
//! would, so only network rules have an effect. `easyprivacy`-style rules and
//! `$domain=` scoping work normally.

use std::io::Read as _;
use std::sync::{Arc, OnceLock, RwLock};
use std::time::Duration;

use adblock::lists::{FilterSet, ParseOptions};
use adblock::request::Request;
use adblock::Engine;

/// Lists built into the executable.
const BUNDLED: &[(&str, &str)] = &[
    ("chrust-adhosts", include_str!("../assets/filters/chrust-adhosts.txt")),
    ("chrust-curated", include_str!("../assets/filters/chrust-curated.txt")),
];

/// Lists downloaded when `adblock_upstream` is on. EasyList and EasyPrivacy
/// are the two uBlock Origin and Adblock Plus use; on their own they block
/// the great majority of ads and trackers.
const UPSTREAM: &[&str] = &[
    "https://easylist.to/easylist/easylist.txt",
    "https://easylist.to/easylist/easyprivacy.txt",
];

/// How long a blank request may wait for the first build. Pages already
/// built the engine for the last run; the first run of a fresh install has to
/// compile the rules, which takes a moment on a cold cache.
const READY_WAIT: Duration = Duration::from_millis(3_000);

static ENGINE: RwLock<Option<Arc<Engine>>> = RwLock::new(None);
static BUILDING: std::sync::Mutex<()> = std::sync::Mutex::new(());
/// Whether the setting is on. Set from the live settings by the browser, so a
/// request never waits on the settings file.
static ENABLED: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(true);

/// Applies the user's choice. Called at start and whenever Settings › Privacy
/// changes, so switching ad blocking off takes effect on the next request.
pub fn set_enabled(on: bool) {
    ENABLED.store(on, std::sync::atomic::Ordering::Relaxed);
}

fn enabled() -> bool {
    ENABLED.load(std::sync::atomic::Ordering::Relaxed)
}

/// The engine, if one has been built.
fn current() -> Option<Arc<Engine>> {
    ENGINE.read().unwrap_or_else(|e| e.into_inner()).clone()
}

/// Starts building the engine in the background, unless it already is.
///
/// `upstream` asynchronously downloads and adds the remote lists. Called at
/// start, when the settings say so.
pub fn warm(upstream: bool) {
    if upstream {
        reload(true);
    } else if current().is_none() {
        reload(false);
    }
}

/// Rebuilds the engine in the background: from the bundled lists alone, or
/// with the upstream lists on top. Used when the setting changes, so ad
/// blocking starts (or stops) using the full lists without a restart.
///
/// A rebuild replaces the live engine in one step; pages that are already
/// loading keep the rules they started with, and every request after the
/// replacement uses the new ones.
pub fn reload(upstream: bool) {
    std::thread::spawn(move || {
        // Serialize rebuilds, so two clicks do not compile twice.
        let _guard = BUILDING.lock().unwrap_or_else(|e| e.into_inner());
        let mut sources: Vec<(&str, String)> = Vec::new();
        if upstream {
            for url in UPSTREAM {
                match download(url) {
                    Ok(text) if text.len() > 1024 => sources.push((url, text)),
                    Ok(_) => log::warn!("adblock: {url} answered with almost nothing; keeping it out"),
                    Err(err) => log::warn!("adblock: could not download {url}: {err}"),
                }
            }
            // A failure to download must not turn blocking off: keep the
            // engine that is working.
            if sources.is_empty() && current().is_some() {
                return;
            }
        }
        install(build(&sources));
    });
}

/// Replaces the live engine. A page that already asked gets the engine that
/// was there at the time, which is why the pointer is shared rather than
/// borrowed from the lock while matching.
fn install(engine: Arc<Engine>) {
    log::info!("adblock: engine rebuilt");
    *ENGINE.write().unwrap_or_else(|e| e.into_inner()) = Some(engine);
}

/// Downloads a list through the configured proxy, with a cap so a hostile or
/// runaway server cannot fill memory.
fn download(url: &str) -> Result<String, String> {
    let response = crate::proxy::agent()
        .get(url)
        .set("User-Agent", "Chrust ad blocking")
        .call()
        .map_err(|err| err.to_string())?;
    let mut text = String::new();
    response
        .into_reader()
        .take(64 * 1024 * 1024)
        .read_to_string(&mut text)
        .map_err(|err| err.to_string())?;
    Ok(text)
}

/// Parses and optimizes the bundled lists plus whatever was downloaded.
///
/// Kept separate from `warm` so tests can build an engine synchronously.
pub fn build(upstream: &[(&str, String)]) -> Arc<Engine> {
    let mut set = FilterSet::new(false);
    for (name, text) in BUNDLED {
        log::debug!("adblock: loading {name}");
        set.add_filter_list(text.to_string(), ParseOptions::default());
    }
    for (url, text) in upstream {
        log::debug!("adblock: loading {url} ({} bytes)", text.len());
        set.add_filter_list(text.clone(), ParseOptions::default());
    }
    Arc::new(Engine::new_with_filter_set(set))
}

/// The engine to ask about requests, waiting briefly for a build in progress.
fn ready() -> Option<Arc<Engine>> {
    if let Some(engine) = current() {
        return Some(engine);
    }
    // A build is probably running (started from `main`); give it a moment
    // before making every request go through.
    std::thread::sleep(READY_WAIT);
    current()
}

/// What kind of resource a WebView2 request carries, as the filter syntax
/// names it (`script`, `image`, `xmlhttprequest`, ...).
///
/// `context` is a `COREWEBVIEW2_WEB_RESOURCE_CONTEXT` value. The number is
/// what has to be matched: the generated type's `Debug` prints
/// `COREWEBVIEW2_WEB_RESOURCE_CONTEXT(6)`, not `Script`, so comparing names
/// silently turned every request into `other` and skipped the rules that are
/// scoped to a type.
pub fn request_type(context: i32) -> &'static str {
    match context {
        1 => "document",
        2 => "stylesheet",
        3 => "image",
        4 => "media",
        5 => "font",
        6 => "script",
        // XMLHttpRequest and fetch are the same thing to a filter list.
        7 | 8 => "xmlhttprequest",
        11 => "websocket",
        14 => "ping",
        // Text tracks, event sources, manifests, signed exchanges and
        // CSP reports have no type of their own in the syntax.
        _ => "other",
    }
}

/// Whether `url`, loaded for `source` (the page asking), should be stopped.
///
/// `private` pages block even when the setting is off, so a private window
/// never shows what the profile has seen.
pub fn should_block(url: &str, source: &str, request_type: &str, private: bool) -> bool {
    if !private && !enabled() {
        return false;
    }
    // The sign-in flow itself must never be interrupted: an ad-like URL in a
    // redirect chain (Google's own consent and OAuth pages use a few) would
    // turn a login into a dead end. See `signin`.
    if crate::signin::is_sign_in_window(url) || crate::signin::is_sign_in_window(source) {
        return false;
    }
    let Some(engine) = ready() else { return false };
    let Ok(request) = Request::new(url, source, request_type, "get") else {
        return false;
    };
    let blocked = engine.check_network_request(&request).should_block();
    if blocked {
        // Rules that match by path can break a page in ways that are hard to
        // guess; naming them on the blocked line makes `RUST_LOG=debug`
        // enough to find the culprit.
        let rules = explain(url, source);
        log::debug!("adblock rule(s): {}", if rules.is_empty() { "?".into() } else { rules.join(" | ") });
    }
    blocked
}

/// Why `should_block` keeps failing, for the log: the first rules that match
/// an address, so a page that breaks can be traced to a list.
pub fn explain(url: &str, source: &str) -> Vec<String> {
    let Some(engine) = ready() else { return Vec::new() };
    let Ok(request) = Request::new(url, source, "other", "get") else {
        return Vec::new();
    };
    let result = engine.check_network_request(&request);
    [&result.exception, &result.filter]
        .into_iter()
        .flatten()
        .filter_map(|rule| rule.raw_line.clone())
        .collect()
}

/// What the settings page shows about the engine.
pub fn stats() -> Stats {
    Stats { ready: current().is_some() }
}

/// How many rules the bundled lists contain, for the settings page. Counts
/// the rule lines as the loader does: comments (`!`) and list headers (`[`)
/// are not rules, and neither are cosmetic rules, which WebView2 cannot act
/// on, so the number is what actually guards a page.
pub fn built_in_rules() -> usize {
    static COUNT: OnceLock<usize> = OnceLock::new();
    *COUNT.get_or_init(|| {
        BUNDLED
            .iter()
            .map(|(_, text)| {
                text.lines()
                    .filter(|line| {
                        let line = line.trim();
                        !line.is_empty()
                            && !line.starts_with('!')
                            && !line.starts_with('[')
                            && !line.contains("##")
                    })
                    .count()
            })
            .sum()
    })
}

#[derive(Clone, Copy, Debug, Default)]
pub struct Stats {
    pub ready: bool,
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Builds the bundled rules once for the whole test binary.
    fn engine() -> Arc<Engine> {
        static TEST_ENGINE: OnceLock<Arc<Engine>> = OnceLock::new();
        TEST_ENGINE.get_or_init(|| build(&[])).clone()
    }

    fn blocks(url: &str, source: &str, kind: &str) -> bool {
        let request = Request::new(url, source, kind, "get").expect("valid url");
        engine().check_network_request(&request).should_block()
    }

    #[test]
    fn blocks_google_ad_delivery() {
        let page = "https://www.youtube.com/watch?v=abc";
        assert!(blocks("https://pubads.g.doubleclick.net/gampad/ads?iu=x", page, "xmlhttprequest"));
        assert!(blocks("https://securepubads.g.doubleclick.net/tag/js/gpt.js", page, "script"));
        assert!(blocks("https://googleads.g.doubleclick.net/pagead/id", page, "xmlhttprequest"));
        assert!(blocks("https://static.doubleclick.net/instream/ad_status.js", page, "script"));
        assert!(blocks("https://www.googletagmanager.com/gtag/js?id=x", "https://example.com/", "script"));
    }

    #[test]
    fn blocks_youtube_ad_endpoints() {
        let page = "https://www.youtube.com/watch?v=abc";
        assert!(blocks("https://www.youtube.com/api/stats/ads?ver=2&x=1", page, "xmlhttprequest"));
        assert!(blocks("https://www.youtube.com/get_midroll_info?x=1", page, "xmlhttprequest"));
        assert!(blocks("https://www.youtube.com/ptracking?x=1", page, "image"));
    }

    #[test]
    fn leaves_pages_and_video_alone() {
        let page = "https://www.youtube.com/watch?v=abc";
        // The video itself and the APIs the player needs to play it.
        assert!(!blocks("https://www.youtube.com/watch?v=abc", page, "document"));
        assert!(!blocks("https://www.youtube.com/youtubei/v1/player?key=x", page, "xmlhttprequest"));
        assert!(!blocks("https://rr1---sn-abcd.googlevideo.com/videoplayback?x=1", page, "media"));
        assert!(!blocks("https://i.ytimg.com/vi/abc/hqdefault.jpg", page, "image"));
        assert!(!blocks("https://www.youtube.com/", "https://www.youtube.com/", "document"));
        assert!(!blocks("https://accounts.google.com/signin", "https://www.google.com/", "document"));
        assert!(!blocks("https://www.google.com/search?q=rust", "https://www.google.com/", "document"));
    }

    #[test]
    fn leaves_ambiguous_first_party_urls_alone() {
        // "ads" appears in the path but the host is the site itself.
        assert!(!blocks("https://www.youtube.com/adsense", "https://www.youtube.com/", "document"));
        // An image on a news site that merely mentions an ad size.
        assert!(!blocks("https://example.com/ad-banner.jpg", "https://example.com/", "image"));
    }

    #[test]
    fn reads_resource_contexts() {
        // COREWEBVIEW2_WEB_RESOURCE_CONTEXT values.
        assert_eq!(request_type(6), "script");
        assert_eq!(request_type(7), "xmlhttprequest");
        assert_eq!(request_type(8), "xmlhttprequest");
        assert_eq!(request_type(3), "image");
        assert_eq!(request_type(4), "media");
        assert_eq!(request_type(1), "document");
        assert_eq!(request_type(0), "other");
        assert_eq!(request_type(16), "other");
    }

    #[test]
    fn built_in_lists_are_present() {
        // A guard against a truncated or empty asset: the executable would
        // still build, and silently stop blocking.
        let rules = built_in_rules();
        assert!(rules > 3_000, "only {rules} built-in rules");
        assert!(blocks("https://www.youtube.com/api/stats/ads", "https://www.youtube.com/", "xmlhttprequest"));
    }

    #[test]
    fn never_interrupts_google_sign_in() {
        // Signing in must survive whatever the lists say about the URLs in
        // the flow, including the tracking-ish ones a consent page loads.
        let source = "https://accounts.google.com/o/oauth2/v2/auth?client_id=x";
        assert!(!should_block("https://www.googletagmanager.com/gtag/js?id=x", source, "script", true));
        assert!(!should_block(
            "https://ssl.gstatic.com/accounts/ui/base.css",
            "https://accounts.google.com/signin",
            "stylesheet",
            true,
        ));
    }
}
