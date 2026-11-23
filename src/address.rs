//! Turning what the user typed in the address bar into a URL, and back.

use url::Url;

/// Built-in pages. WebView2 serves the custom `aurora` protocol at
/// `http://aurora.<page>/`; the address bar shows them as `aurora://<page>`.
pub const INTERNAL_PAGES: &[&str] = &["newtab", "settings"];

pub const NEW_TAB_URL: &str = "http://aurora.newtab/";
pub const SETTINGS_URL: &str = "http://aurora.settings/";

/// Converts address bar input into a URL to load, or `None` for blank input.
/// `search` is a search URL template where `{}` stands for the query.
pub fn to_url(input: &str, search: &str) -> Option<String> {
    let input = input.trim();
    if input.is_empty() {
        return None;
    }

    if let Some(page) = internal_name(input) {
        return Some(format!("http://aurora.{page}/"));
    }

    // Windows paths such as `C:\Users\me\page.html`.
    if is_windows_path(input) {
        if let Ok(url) = Url::from_file_path(input) {
            return Some(url.to_string());
        }
    }

    if input.contains(char::is_whitespace) {
        return Some(search_url(search, input));
    }

    // Explicit schemes: https://, file://, about:blank, view-source: ...
    if let Ok(url) = Url::parse(input) {
        let scheme = url.scheme();
        let known = matches!(
            scheme,
            "http" | "https" | "file" | "about" | "data" | "mailto" | "view-source" | "edge" | "blob"
        );
        if known && (input.contains("://") || scheme != "http" && scheme != "https") {
            return Some(url.to_string());
        }
    }

    if looks_like_host(input) {
        let scheme = if is_local(input) { "http" } else { "https" };
        if let Ok(url) = Url::parse(&format!("{scheme}://{input}")) {
            return Some(url.to_string());
        }
    }

    Some(search_url(search, input))
}

/// `aurora://settings`, `about:newtab` → `settings`, `newtab`.
fn internal_name(input: &str) -> Option<&'static str> {
    let lower = input.to_ascii_lowercase();
    let name = lower
        .strip_prefix("aurora://")
        .or_else(|| lower.strip_prefix("about:"))
        .or_else(|| lower.strip_prefix("http://aurora.").or_else(|| lower.strip_prefix("aurora.")))?;
    let name = name.trim_end_matches('/');
    INTERNAL_PAGES.iter().copied().find(|p| *p == name)
}

/// The built-in page name if `url` is one, e.g. `settings`.
pub fn internal_page(url: &str) -> Option<&'static str> {
    let rest = url.strip_prefix("http://aurora.")?;
    let name = rest.split(['/', '?', '#']).next()?;
    INTERNAL_PAGES.iter().copied().find(|p| *p == name)
}

pub fn is_internal(url: &str) -> bool {
    internal_page(url).is_some()
}

/// Text to show in the address bar for a loaded URL.
pub fn display(url: &str) -> String {
    match internal_page(url) {
        Some("newtab") => String::new(),
        Some(page) => format!("aurora://{page}"),
        None if url == "about:blank" => String::new(),
        None => url.to_string(),
    }
}

pub fn is_new_tab(url: &str) -> bool {
    internal_page(url) == Some("newtab")
}

/// Whether the page was loaded over a secure connection.
pub fn is_secure(url: &str) -> bool {
    url.starts_with("https://")
}

pub fn search_url(template: &str, query: &str) -> String {
    let encoded: String = url::form_urlencoded::byte_serialize(query.as_bytes()).collect();
    template.replace("{}", &encoded)
}

fn is_windows_path(input: &str) -> bool {
    let b = input.as_bytes();
    (b.len() >= 3 && b[0].is_ascii_alphabetic() && b[1] == b':' && (b[2] == b'\\' || b[2] == b'/'))
        || input.starts_with("\\\\")
}

/// Host part of `input`, without path, query or port.
fn host_of(input: &str) -> &str {
    let end = input.find(['/', '?', '#']).unwrap_or(input.len());
    let authority = &input[..end];
    let authority = authority.rsplit('@').next().unwrap_or(authority);
    if authority.starts_with('[') {
        // IPv6 literal: keep the brackets, drop the port.
        return authority.split(']').next().map_or(authority, |h| &authority[..h.len() + 1]);
    }
    authority.split(':').next().unwrap_or(authority)
}

fn is_local(input: &str) -> bool {
    let host = host_of(input);
    host.eq_ignore_ascii_case("localhost")
        || host.ends_with(".localhost")
        || host.parse::<std::net::Ipv4Addr>().is_ok()
        || host.starts_with('[')
}

fn looks_like_host(input: &str) -> bool {
    let host = host_of(input);
    if host.is_empty() {
        return false;
    }
    if is_local(input) {
        return true;
    }
    // Needs a dot with something on both sides, e.g. `example.com`.
    let Some((name, tld)) = host.rsplit_once('.') else {
        return false;
    };
    !name.is_empty()
        && tld.len() >= 2
        && tld.chars().all(|c| c.is_ascii_alphabetic())
        && host.chars().all(|c| c.is_alphanumeric() || c == '-' || c == '.')
}

#[cfg(test)]
mod tests {
    use super::*;

    const G: &str = "https://www.google.com/search?q={}";

    #[test]
    fn blank_input() {
        assert_eq!(to_url("   ", G), None);
    }

    #[test]
    fn full_urls_pass_through() {
        assert_eq!(to_url("https://example.com/a?b=1", G).unwrap(), "https://example.com/a?b=1");
        assert_eq!(to_url("http://example.com", G).unwrap(), "http://example.com/");
        assert_eq!(to_url("about:blank", G).unwrap(), "about:blank");
    }

    #[test]
    fn bare_domains_get_https() {
        assert_eq!(to_url("example.com", G).unwrap(), "https://example.com/");
        assert_eq!(to_url("en.wikipedia.org/wiki/Rust", G).unwrap(), "https://en.wikipedia.org/wiki/Rust");
    }

    #[test]
    fn local_addresses_get_http() {
        assert_eq!(to_url("localhost:3000", G).unwrap(), "http://localhost:3000/");
        assert_eq!(to_url("127.0.0.1:8080/x", G).unwrap(), "http://127.0.0.1:8080/x");
    }

    #[test]
    fn everything_else_is_a_search() {
        assert_eq!(to_url("rust gpui", G).unwrap(), "https://www.google.com/search?q=rust+gpui");
        assert_eq!(to_url("c++", G).unwrap(), "https://www.google.com/search?q=c%2B%2B");
        assert_eq!(
            to_url("hello", "https://duckduckgo.com/?q={}").unwrap(),
            "https://duckduckgo.com/?q=hello"
        );
    }

    #[test]
    fn windows_paths_become_file_urls() {
        assert_eq!(to_url(r"C:\pages\index.html", G).unwrap(), "file:///C:/pages/index.html");
    }

    #[test]
    fn internal_pages() {
        assert_eq!(to_url("aurora://newtab", G).unwrap(), NEW_TAB_URL);
        assert_eq!(to_url("aurora://settings/", G).unwrap(), SETTINGS_URL);
        assert_eq!(to_url("about:settings", G).unwrap(), SETTINGS_URL);
        assert_eq!(to_url("aurora://nope", G).unwrap(), "https://www.google.com/search?q=aurora%3A%2F%2Fnope");
        assert!(is_new_tab("http://aurora.newtab"));
        assert_eq!(internal_page("http://aurora.settings/#about"), Some("settings"));
        assert_eq!(display(NEW_TAB_URL), "");
        assert_eq!(display(SETTINGS_URL), "aurora://settings");
        assert_eq!(display("https://a.com/"), "https://a.com/");
    }
}
