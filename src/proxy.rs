//! A proxy for everything Chrust fetches.
//!
//! Two things can point at a SOCKS5 or HTTP proxy (for example
//! `socks5://127.0.0.1:9150` or `127.0.0.1:9150`):
//!
//! - Settings › Privacy › VPN: an address saved in `settings.json`, changed
//!   from inside the browser and applied on the next start.
//! - The environment variable `CHRUST_PROXY`, which is read when it is set
//!   and otherwise falls back to the saved address.
//!
//! Whichever wins, the page engine and Chrust's own downloads (extensions,
//! updates, site icons) all go through it; with neither, everything connects
//! directly.

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
    let valid_host = valid_host(&host);
    let port_number: u16 = port.parse().ok()?;
    if !valid_host || port_number == 0 {
        return None;
    }
    Some(if kind.starts_with("socks") { Proxy::Socks5(host, port) } else { Proxy::Http(host, port) })
}

fn valid_host(host: &str) -> bool {
    let plain = host.trim_start_matches('[').trim_end_matches(']');
    !plain.is_empty()
        && plain.chars().all(|c| c.is_ascii_alphanumeric() || c == '.' || c == '-' || c == ':')
}

/// Checks an address typed into the settings page, in the same shape
/// [`resolve`] uses: `scheme://host:port`. An empty address means "no proxy".
pub fn validate_address(input: &str) -> Result<(), &'static str> {
    let input = input.trim();
    if input.is_empty() {
        return Ok(());
    }
    if input.chars().any(char::is_whitespace) {
        return Err("An address cannot contain spaces.");
    }
    let rest = input;
    match rest.split_once("://") {
        None => return Err("Write the address like socks5://127.0.0.1:1080."),
        Some((kind, _)) if !kind.eq_ignore_ascii_case("socks5")
            && !kind.eq_ignore_ascii_case("socks")
            && !kind.eq_ignore_ascii_case("http")
            && !kind.eq_ignore_ascii_case("https") => Err("Start the address with socks5:// or http://."),
        _ => match parse(rest) {
            Some(_) => Ok(()),
            None => Err("That is not an address like socks5://127.0.0.1:1080."),
        },
    }
}

/// True when [`resolve`] would use this saved address. Spelled-out addresses
/// only: plain text in the settings file (an old hand edit, or a VPN app's
/// name) is left alone rather than turned into a tunnel to a guessed port.
pub fn is_usable_address(input: &str) -> bool {
    let address = input.trim();
    address.is_empty() || address.contains("://")
}

/// Read by the command that runs the VPN's own program, so a worked address
/// is the one that gets saved.
pub fn parse_address(input: &str) -> Option<String> {
    validate_address(input).ok()?;
    parse(input).map(|proxy| proxy.label())
}

/// The proxy this run uses: `CHRUST_PROXY` if set, else the one saved in
/// settings. It is fixed when the browser starts, because the page engine
/// only takes it on start-up; changing it in Settings asks for a restart.
pub fn configured() -> Option<&'static Proxy> {
    static RESOLVED: OnceLock<Option<Proxy>> = OnceLock::new();
    RESOLVED.get_or_init(resolve).as_ref()
}

fn resolve() -> Option<Proxy> {
    let address = std::env::var("CHRUST_PROXY")
        .ok()
        .filter(|value| !value.trim().is_empty())
        .unwrap_or_else(|| crate::storage::load_settings().proxy);
    // An address spelled out with a scheme and a port is used as it is:
    // the user said which tunnel they mean. Plain text (a VPN app's name,
    // a bare address) is not, because the default ports would turn it into
    // a tunnel to a port nothing listens on, and then every page fails with
    // no clue why.
    let proxy = parse(&address).filter(|_| address.trim().is_empty() || is_usable_address(&address));
    if !address.trim().is_empty() && !is_usable_address(&address) {
        log::warn!("VPN address {address:?} is not a scheme://host:port address; ignoring it");
        return None;
    }
    if let Some(proxy) = &proxy {
        log::info!("using proxy {}", proxy.label());
    }
    proxy
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
    /// makes the loopback proxy on this machine usable at all (a VPN app
    /// keeps its tunnel on 127.0.0.1).
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

    /// For log lines and for saving back to settings.
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

/// The local address common VPN apps open on Windows, offered on the
/// settings page as a starting point. Chrust cannot log in to a VPN by
/// itself; these are the tunnels those apps leave running.
pub const PRESETS: &[(&str, &str)] = &[
    ("Mullvad", "socks5://127.0.0.1:1080"),
    ("Proton VPN", "socks5://127.0.0.1:9050"),
    ("NordVPN", "socks5://127.0.0.1:1080"),
    ("Surfshark", "socks5://127.0.0.1:1080"),
    ("WireGuard (wg-socks)", "socks5://127.0.0.1:8080"),
    ("Tor Browser", "socks5://127.0.0.1:9150"),
    ("Tor service", "socks5://127.0.0.1:9050"),
    ("Custom", ""),
];

/// The address the engine is actually using, for the settings page; empty
/// when pages connect directly.
pub fn active_label() -> String {
    configured().map(Proxy::label).unwrap_or_default()
}

/// True when `CHRUST_PROXY` outranks the saved setting for this launch, so
/// the settings page can say why a change waits for the next start.
pub fn env_override() -> bool {
    std::env::var("CHRUST_PROXY").is_ok_and(|value| !value.trim().is_empty())
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
        // Ports the engine could not use are refused rather than handed on.
        assert_eq!(parse("127.0.0.1:0"), None);
        assert_eq!(parse("127.0.0.1:70000"), None);
    }

    #[test]
    fn validates_settings_addresses() {
        assert!(validate_address("").is_ok());
        assert!(validate_address("   ").is_ok());
        assert!(validate_address("socks5://127.0.0.1:9150").is_ok());
        assert!(validate_address("SOCKS5://127.0.0.1:9150").is_ok());
        assert!(validate_address("http://127.0.0.1:8080").is_ok());
        assert!(validate_address("socks5://[::1]:9150").is_ok());
        // A scheme is required, so a guessed default port never becomes a
        // tunnel to nothing.
        assert!(validate_address("127.0.0.1:1080").is_err());
        assert!(validate_address("127.0.0.1").is_err());
        assert!(validate_address("socks5://127.0.0.1:9150 --proxy-bypass-list=x").is_err());
        assert!(validate_address("socks5://127.0.0.1:zero").is_err());
        assert!(validate_address("ftp://127.0.0.1:21").is_err());
        assert_eq!(parse_address(" socks5://127.0.0.1:9150 "), Some("socks5://127.0.0.1:9150".into()));
        assert_eq!(parse_address("127.0.0.1:9150"), None);
        assert!(is_usable_address("socks5://127.0.0.1:1080"));
        assert!(!is_usable_address("127.0.0.1:1080"));
        assert!(is_usable_address(""));
    }
}
