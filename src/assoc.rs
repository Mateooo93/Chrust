//! Registering Chrust as a browser, so Windows offers it in "Default apps"
//! and every other program can hand it a link.
//!
//! Everything is per user (`HKCU`), so this needs no administrator and is
//! undone by running the same command with `--unregister-browser`. Windows 11
//! will not let a program *make itself* the default; registering puts Chrust in
//! the list, and the user picks it in Settings (the settings page links there).
//!
//! The registry shape is the one Windows documents for a browser:
//!
//! - `Software\Clients\StartMenuInternet\ChrustPort` – the client itself, with
//!   its icon and its shell command.
//! - The same key's `Capabilities` – what Chrust handles: `http` and `https`.
//!   `Software\RegisteredApplications` then points "Default apps" at them.
//! - `Software\Classes\ChrustPortHTM` – the `ProgId` a launched Chrust window
//!   reports, with the command that opens a URL in it.
//!
//! The command is `<exe> "%1"`. Clicked while Chrust is open, that would put a
//! second process on the same WebView2 profile, so the pushed-to window
//! notices the running copy and writes the URL for it to pick up instead
//! (`handoff`); a click in a served page's own popup stays inside WebView2.

use std::path::Path;

pub const PROG_ID: &str = "ChrustPortHTM";
pub const CLIENT: &str = "ChrustPort";
pub const APP_NAME: &str = "ChrustPort";

const REGISTER_FLAG: &str = "--register-browser";
const UNREGISTER_FLAG: &str = "--unregister-browser";

/// The `http(s)` handler Windows calls when a link is opened.
pub fn open_command(exe: &Path) -> String {
    format!("\"{}\" \"%1\"", exe.display())
}

/// True when this launch registers or unregisters instead of opening a page.
pub fn is_admin_request(args: &[String]) -> bool {
    args.iter().any(|a| a.eq_ignore_ascii_case(REGISTER_FLAG) || a.eq_ignore_ascii_case(UNREGISTER_FLAG))
}

/// True for `--register-browser`; the two flags are never both acted on.
pub fn wants_register(args: &[String]) -> bool {
    args.iter().any(|a| a.eq_ignore_ascii_case(REGISTER_FLAG))
}

pub fn wants_unregister(args: &[String]) -> bool {
    args.iter().any(|a| a.eq_ignore_ascii_case(UNREGISTER_FLAG))
}

#[cfg(windows)]
mod win {
    use super::*;

    pub fn set(hkcu: &winreg::RegKey, key: &str, values: &[(&str, String)]) -> Result<(), String> {
        let (root, _) = hkcu
            .create_subkey(key)
            .map_err(|e| format!("could not create HKCU\\{key}: {e}"))?;
        for (name, data) in values {
            root.set_value(name, data).map_err(|e| format!("could not set HKCU\\{key}\\{name}: {e}"))?;
        }
        Ok(())
    }

    pub fn register(exe: &Path) -> Result<(), String> {
        let hkcu = winreg::RegKey::predef(winreg::enums::HKEY_CURRENT_USER);
        let ui = exe.display().to_string();
        let icon = format!("{ui},0");
        let command = open_command(exe);

        let client = format!("Software\\Clients\\StartMenuInternet\\{CLIENT}");
        set(&hkcu, &client, &[("", APP_NAME.to_string())])?;
        set(
            &hkcu,
            &format!("{client}\\Capabilities"),
            &[
                ("ApplicationName".into(), APP_NAME.to_string()),
                ("ApplicationDescription".into(), "A fast browser with a native Rust interface".into()),
                ("ApplicationIcon".into(), icon.clone()),
            ],
        )?;
        set(
            &hkcu,
            &format!("{client}\\Capabilities\\URLAssociations"),
            &[("http".into(), PROG_ID.to_string()), ("https".into(), PROG_ID.to_string())],
        )?;
        set(&hkcu, &format!("{client}\\DefaultIcon"), &[("".into(), icon.clone())])?;
        set(&hkcu, &format!("{client}\\shell\\open\\command"), &[("".into(), command.clone())])?;
        set(
            &hkcu,
            "Software\\RegisteredApplications",
            &[(APP_NAME, format!("{client}\\Capabilities"))],
        )?;

        let prog_id = format!("Software\\Classes\\{PROG_ID}");
        set(&hkcu, &prog_id, &[("".into(), "Chrust HTML Document".into())])?;
        set(&hkcu, &format!("{prog_id}\\DefaultIcon"), &[("".into(), icon)])?;
        set(&hkcu, &format!("{prog_id}\\shell\\open\\command"), &[("".into(), command)])?;

        // Listed as an option on each scheme, so the user can pick Chrust even
        // before it is the default.
        for scheme in ["http", "https"] {
            set(
                &hkcu,
                &format!("Software\\Classes\\{scheme}\\OpenWithProgids"),
                &[(PROG_ID, String::new())],
            )?;
        }
        Ok(())
    }

    pub fn unregister() -> Result<(), String> {
        let hkcu = winreg::RegKey::predef(winreg::enums::HKEY_CURRENT_USER);
        // Only Chrust's own keys: uninstalling must never disturb another
        // browser's association. A key that is already gone is the normal
        // case on a fresh machine.
        for key in [
            format!("Software\\Clients\\StartMenuInternet\\{CLIENT}"),
            format!("Software\\Classes\\{PROG_ID}"),
        ] {
            let (parent, leaf) = key.rsplit_once('\\').unwrap_or(("", key.as_str()));
            if let Ok(root) = hkcu.open_subkey(parent) {
                let _ = root.delete_subkey_all(leaf);
            }
        }
        for scheme in ["http", "https"] {
            if let Ok(key) = hkcu.open_subkey(format!("Software\\Classes\\{scheme}\\OpenWithProgids")) {
                let _ = key.delete_value(PROG_ID);
            }
        }
        if let Ok(key) = hkcu.open_subkey("Software\\RegisteredApplications") {
            let _ = key.delete_value(APP_NAME);
        }
        Ok(())
    }
}

#[cfg(windows)]
pub use win::{register, unregister};

/// True when "Default apps" already lists Chrust, so the settings page can say
/// so rather than offering to register again.
#[cfg(windows)]
pub fn is_registered() -> bool {
    use winreg::RegKey;
    use winreg::enums::HKEY_CURRENT_USER;
    RegKey::predef(HKEY_CURRENT_USER)
        .open_subkey("Software\\RegisteredApplications")
        .and_then(|key| key.get_value::<String, _>(APP_NAME))
        .is_ok()
}

#[cfg(not(windows))]
pub fn is_registered() -> bool {
    false
}

#[cfg(not(windows))]
pub fn register(_exe: &Path) -> Result<(), String> {
    Err("only Windows offers a default browser this way".into())
}

#[cfg(not(windows))]
pub fn unregister() -> Result<(), String> {
    Err("only Windows offers a default browser this way".into())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn args(list: &[&str]) -> Vec<String> {
        list.iter().map(|s| s.to_string()).collect()
    }

    #[test]
    fn command_quotes_the_exe_and_passes_the_url() {
        assert_eq!(
            open_command(Path::new(r"C:\Program Files\Chrust\chrust-port.exe")),
            r#""C:\Program Files\Chrust\chrust-port.exe" "%1""#
        );
    }

    #[test]
    fn flags_are_asked_for_and_recognised_case_insensitively() {
        assert!(wants_register(&args(&["--Register-Browser"])));
        assert!(!wants_unregister(&args(&["--Register-Browser"])));
        assert!(wants_unregister(&args(&["--unregister-browser"])));
        assert!(is_admin_request(&args(&["--unregister-browser"])));
        // A normal launch, and a link, are neither.
        assert!(!is_admin_request(&args(&["https://example.com/"])));
        assert!(!is_admin_request(&[]));
    }
}
