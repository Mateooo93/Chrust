//! Google account sign-in.
//!
//! Signing in to Google, and to any site that offers "Sign in with Google",
//! happens in a popup that reports back to the page that opened it through
//! `window.opener`. WebView2 keeps the opener when it opens that popup itself,
//! which is what `Allow` asks for, so a sized popup is left to the engine
//! instead of being turned into an ordinary tab (see the new-window handler in
//! `tab`).
//!
//! This module only answers "is this part of Google's sign-in?". Two things
//! use that answer: ad blocking never touches these hosts, so a rule cannot
//! turn a login into a dead end, and the new-window handler can say in the log
//! which popup it let through.
//!
//! Nothing here reads or stores an account. The login, its cookies and its
//! "keep me signed in" token live in the WebView2 profile the rest of the
//! browser uses, so clearing site data signs out exactly as it does anywhere.

/// Hosts that make up Google's sign-in flow, including the accounts used by a
/// Workspace domain (`accounts.google.com`, `*.gstatic.com` for its scripts,
/// and the `googleusercontent.com` host that serves the signed-in avatar).
///
/// Matched as host suffix, so `accounts.google.com` and
/// `accounts.google.co.uk` both qualify, and a look-alike such as
/// `accounts.google.com.evil.example` does not.
const SIGN_IN_HOSTS: &[&str] = &[
    "accounts.google.com",
    "accounts.google.co.uk",
    "accounts.googleusercontent.com",
    "accounts.youtube.com",
    "consent.google.com",
    "consent.youtube.com",
    "oauth2.googleapis.com",
    "gstatic.com",
    "googleusercontent.com",
];

fn host_of(url: &str) -> Option<String> {
    let rest = url.strip_prefix("https://").or_else(|| url.strip_prefix("http://"))?;
    let authority = rest.split(['/', '?', '#']).next()?;
    // Drop a user name and port: `user:pass@host:443`.
    let authority = authority.rsplit('@').next()?;
    let host = authority.split(':').next()?.to_ascii_lowercase();
    if host.is_empty() || !host.contains('.') {
        return None;
    }
    Some(host)
}

fn on_host(host: &str, suffix: &str) -> bool {
    host == suffix || host.ends_with(&format!(".{suffix}"))
}

/// Whether a URL belongs to Google's sign-in flow.
pub fn is_sign_in_window(url: &str) -> bool {
    // Only the pages that run a login belong here. A signed-in site that just
    // happens to load a Google script keeps its ordinary popup behaviour.
    let Some(host) = host_of(url) else { return false };
    SIGN_IN_HOSTS.iter().any(|suffix| on_host(&host, suffix))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn recognises_google_sign_in_windows() {
        assert!(is_sign_in_window("https://accounts.google.com/gsi/select"));
        assert!(is_sign_in_window("https://accounts.google.com/o/oauth2/v2/auth?client_id=x"));
        assert!(is_sign_in_window("https://consent.google.com/ml?continue=x"));
        assert!(is_sign_in_window("https://accounts.google.co.uk/signin"));
        assert!(is_sign_in_window("https://ssl.gstatic.com/accounts/ui/base.css"));
        assert!(is_sign_in_window("https://lh3.googleusercontent.com/a/photo"));
    }

    #[test]
    fn ignores_look_alikes_and_other_sites() {
        assert!(!is_sign_in_window("https://accounts.google.com.evil.example/signin"));
        assert!(!is_sign_in_window("https://evil-accounts.google.com/"));
        assert!(!is_sign_in_window("https://example.com/signin"));
        assert!(!is_sign_in_window("https://paypal.com/popup"));
        assert!(!is_sign_in_window("javascript:alert(1)"));
        assert!(!is_sign_in_window("data:text/html,hi"));
        assert!(!is_sign_in_window("not a url"));
    }
}
