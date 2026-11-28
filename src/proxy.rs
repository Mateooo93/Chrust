//! A proxy for everything Chrust fetches.
//!
//! The environment variable `CHRUST_PROXY` points at a SOCKS5 or HTTP proxy
//! (for example `socks5://127.0.0.1:9150` or `127.0.0.1:9150`). When it is
//! set, the page engine and Chrust's own downloads (extensions, updates,
//! site icons) all go through it; unset, everything connects directly.

use std::sync::OnceLock;

#[derive(Clone, Debug, PartialEq)]
pub enum Proxy {
    Socks5(String, String),
    Http(String, String),
}

fn parse(input: &str) -> Option<Proxy> {
    let input = input.trim();
    if input.is_empty() {
        return None;
    }
    let (kind, rest) = match input.split_once("://") {
        Some((kind, rest)) => (kind.to_ascii_lowercase(), rest),
        None => ("socks5".to_string(), input),
    };
    let rest = rest.trim_end_matches('/');
    // IPv6 addresses keep their brackets, so the colon inside them is not
    // mistaken for the port separator: [::1]:9050.
    let (host, port) = if let Some(inner) = rest.strip_prefix('[') {
        match inner.split_once("]:") {
            Some((host, port)) => (format!("[{host}]"), port.to_string()),
            None => return None,
        }
    } else {
        match rest.rsplit_once(':') {
            Some((host, port)) => (host.to_string(), port.to_string()),
        // A proxy without a port uses its usual one.
            None if kind.starts_with("socks") => (rest.to_string(), "1080".to_string()),
            None => (rest.to_string(), "8080".to_string()),
        }
    };
    let plain = host.trim_start_matches('[').trim_end_matches(']');
    let valid_host = !plain.is_empty()
        && plain.chars().all(|c| c.is_ascii_alphanumeric() || c == '.' || c == '-' || c == ':');
    if !valid_host || port.parse::<u16>().is_err() {
        return None;
    }
    Some(if kind.starts_with("socks") { Proxy::Socks5(host, port) } else { Proxy::Http(host, port) })
}

/// The proxy this run uses, read once from the environment.
pub fn configured() -> Option<&'static Proxy> {
    static PROXY: OnceLock<Option<Proxy>> = OnceLock::new();
    PROXY
        .get_or_init(|| std::env::var("CHRUST_PROXY").ok().and_then(|value| parse(&value)))
        .as_ref()
}

impl Proxy {
    /// For wry's page engine, on platforms that take it there. The address
    /// is split the way wry expects it back.
    #[allow(dead_code)]
    pub fn for_webview(&self) -> wry::ProxyConfig {
        let endpoint = |host: &str, port: &str| wry::ProxyEndpoint { host: host.to_string(), port: port.to_string() };
        match self {
            Proxy::Socks5(host, port) => wry::ProxyConfig::Socks5(endpoint(host, port)),
            Proxy::Http(host, port) => wry::ProxyConfig::Http(endpoint(host, port)),
        }
    }

    /// Chromium's own switch, given to the page engine as a start-up
    /// argument. Local addresses still go through the proxy, which is what
    /// makes the loopback proxy on this machine usable at all.
    pub fn browser_flag(&self) -> String {
        match self {
            Proxy::Socks5(host, port) => {
                format!("--proxy-server=socks5://{host}:{port} --proxy-bypass-list=<-loopback>")
            }
            Proxy::Http(host, port) => {
                format!("--proxy-server=http://{host}:{port} --proxy-bypass-list=<-loopback>")
            }
        }
    }

    /// For log lines.
    pub fn label(&self) -> String {
        match self {
            Proxy::Socks5(host, port) => format!("socks5://{host}:{port}"),
            Proxy::Http(host, port) => format!("http://{host}:{port}"),
        }
    }

    /// For an HTTP client, as the URL its `Proxy`/`Proxy::Socks5` wants.
    pub fn endpoint(&self) -> (bool, String) {
        match self {
            Proxy::Socks5(host, port) => (true, format!("{host}:{port}")),
            Proxy::Http(host, port) => (false, format!("{host}:{port}")),
        }
    }
}

/// An HTTP client that goes through the configured proxy.
pub fn agent() -> ureq::Agent {
    let builder = ureq::AgentBuilder::new().timeout(std::time::Duration::from_secs(120));
    match configured() {
        Some(proxy) => {
            let (socks, address) = proxy.endpoint();
            if socks {
                builder.proxy(ureq::Proxy::new(&format!("socks5://{address}")).expect("checked when parsed"))
            } else {
                builder.proxy(ureq::Proxy::new(&format!("http://{address}")).expect("checked when parsed"))
            }
        }
        None => builder,
    }
    .build()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reads_proxy_addresses() {
        assert_eq!(parse("socks5://127.0.0.1:9150"), Some(Proxy::Socks5("127.0.0.1".into(), "9150".into())));
        assert_eq!(parse("127.0.0.1:9150"), Some(Proxy::Socks5("127.0.0.1".into(), "9150".into())));
        assert_eq!(parse("http://proxy:3128/"), Some(Proxy::Http("proxy".into(), "3128".into())));
        assert_eq!(parse(" socks5://[::1]:9050 "), Some(Proxy::Socks5("[::1]".into(), "9050".into())));
        assert_eq!(parse(""), None);
        assert_eq!(parse("no-port:"), None);
        assert_eq!(parse("bogus://"), None);
    }
}
