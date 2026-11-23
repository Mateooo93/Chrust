//! Turning what the user typed in the address bar into a URL, and back.

use url::Url;

/// Search engine used for anything that does not look like an address.
const SEARCH_URL: &str = "https://www.google.com/search?q=";

/// The built-in new tab page. WebView2 serves the custom `aurora` protocol
/// at `http://aurora.<path>/`.
pub const NEW_TAB_URL: &str = "http://aurora.newtab/";

/// Converts address bar input into a URL to load, or `None` for blank input.
pub fn to_url(input: &str) -> Option<String> {
    let input = input.trim();
    if input.is_empty() {
        return None;
    }

    if input.eq_ignore_ascii_case("aurora://newtab") || input.eq_ignore_ascii_case("about:newtab") {
        return Some(NEW_TAB_URL.to_string());
    }

    // Windows paths such as `C:\Users\me\page.html`.
    if is_windows_path(input) {
        if let Ok(url) = Url::from_file_path(input) {
            return Some(url.to_string());
        }
    }

    if input.contains(char::is_whitespace) {
        return Some(search_url(input));
    }

    // Explicit schemes: https://, file://, about:blank, mailto:, view-source: ...
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

    Some(search_url(input))
}

/// Text to show in the address bar for a loaded URL.
pub fn display(url: &str) -> String {
    if is_new_tab(url) || url == "about:blank" {
        String::new()
    } else {
        url.to_string()
    }
}

pub fn is_new_tab(url: &str) -> bool {
    url.trim_end_matches('/') == NEW_TAB_URL.trim_end_matches('/')
}

/// Whether the page was loaded over a secure connection.
pub fn is_secure(url: &str) -> bool {
    url.starts_with("https://")
}

fn search_url(query: &str) -> String {
    let encoded: String = url::form_urlencoded::byte_serialize(query.as_bytes()).collect();
    format!("{SEARCH_URL}{encoded}")
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

    #[test]
    fn blank_input() {
        assert_eq!(to_url("   "), None);
    }

    #[test]
    fn full_urls_pass_through() {
        assert_eq!(to_url("https://example.com/a?b=1").unwrap(), "https://example.com/a?b=1");
        assert_eq!(to_url("http://example.com").unwrap(), "http://example.com/");
        assert_eq!(to_url("about:blank").unwrap(), "about:blank");
    }

    #[test]
    fn bare_domains_get_https() {
        assert_eq!(to_url("example.com").unwrap(), "https://example.com/");
        assert_eq!(to_url("en.wikipedia.org/wiki/Rust").unwrap(), "https://en.wikipedia.org/wiki/Rust");
        assert_eq!(to_url("youtube.com").unwrap(), "https://youtube.com/");
    }

    #[test]
    fn local_addresses_get_http() {
        assert_eq!(to_url("localhost:3000").unwrap(), "http://localhost:3000/");
        assert_eq!(to_url("127.0.0.1:8080/x").unwrap(), "http://127.0.0.1:8080/x");
        assert_eq!(to_url("192.168.1.1").unwrap(), "http://192.168.1.1/");
    }

    #[test]
    fn everything_else_is_a_search() {
        assert_eq!(to_url("rust gpui").unwrap(), "https://www.google.com/search?q=rust+gpui");
        assert_eq!(to_url("hello").unwrap(), "https://www.google.com/search?q=hello");
        assert_eq!(to_url("what is 1.5").unwrap(), "https://www.google.com/search?q=what+is+1.5");
        assert_eq!(to_url("c++").unwrap(), "https://www.google.com/search?q=c%2B%2B");
        assert_eq!(to_url("file.txt1").unwrap(), "https://www.google.com/search?q=file.txt1");
    }

    #[test]
    fn windows_paths_become_file_urls() {
        assert_eq!(to_url(r"C:\pages\index.html").unwrap(), "file:///C:/pages/index.html");
    }

    #[test]
    fn new_tab_page() {
        assert_eq!(to_url("aurora://newtab").unwrap(), NEW_TAB_URL);
        assert!(is_new_tab("http://aurora.newtab"));
        assert_eq!(display(NEW_TAB_URL), "");
        assert_eq!(display("https://a.com/"), "https://a.com/");
    }
}
