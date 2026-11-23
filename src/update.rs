//! Checking GitHub for a newer Aurora and installing it.
//!
//! Replacing the running executable is Windows-only; on other platforms the
//! menu entry is hidden and the settings page says to get the new version
//! from the repository.
//!
//! "Check for updates" asks GitHub for the newest release that ships an
//! executable. When its version is newer than this build, the file downloads
//! into `%LOCALAPPDATA%\Aurora\Updates` (and shows up in the downloads panel
//! like any other download), then a small script waits for Aurora to close,
//! swaps the executable in place, and starts it again. Only the executable is
//! replaced: the profile, history and settings stay where they are. If moving
//! the new file fails (a write-protected install folder, for example), it is
//! left beside the old one and shown in Explorer.

use std::fs;
use std::io::Read as _;
use std::path::{Path, PathBuf};
use std::sync::OnceLock;

use gpui_kit::App;

use crate::library::Shared;

/// The repository releases are published from.
pub const REPO: &str = "Mateooo93/aurora";
const API: &str = "https://api.github.com/repos/Mateooo93/aurora/releases/latest";
const SCRIPT_NAME: &str = "aurora-update.cmd";
/// Seconds the restart script waits between checks for Aurora having closed.
const POLL_SECONDS: &str = "2";

fn agent() -> ureq::Agent {
    static AGENT: OnceLock<ureq::Agent> = OnceLock::new();
    AGENT.get_or_init(crate::proxy::agent).clone()
}

/// Where the downloaded build waits for Aurora to close.
#[cfg(windows)]
fn updates_dir() -> PathBuf {
    crate::storage::data_dir().join("Updates")
}

/// Whether this platform can install an update in place.
pub const CAN_INSTALL: bool = cfg!(windows);

/// Asks GitHub for the newest release. Waiting on the network happens on its
/// own thread; the outcome reaches the UI through `cx.spawn`.
pub fn check_and_install(lib: Shared, cx: &mut App) {
    let (tx, rx) = futures::channel::oneshot::channel();
    std::thread::spawn(move || {
        let _ = tx.send(check());
    });
    cx.spawn(async move |cx| {
        let result = rx.await.ok();
        let _ = cx.update(|app| {
            let text = result.unwrap_or_else(|| "The update check did not finish.".into());
            {
                let mut lib = lib.borrow_mut();
                lib.update_message = Some(text);
                lib.flush();
            }
            app.refresh_windows();
        });
    })
    .detach();
}

fn request() -> ureq::Request {
    ureq::get(API)
        .set("Accept", "application/vnd.github+json")
        .set("X-GitHub-Api-Version", "2022-11-28")
        .set("User-Agent", concat!("Aurora/", env!("CARGO_PKG_VERSION")))
}

/// The whole update check and download; returns the text to show.
fn check() -> String {
    let response = match request().call() {
        Ok(response) => response,
        Err(err) => return status_text(err),
    };
    let body: serde_json::Value = match response.into_string() {
        Ok(text) => match serde_json::from_str(&text) {
            Ok(body) => body,
            Err(err) => return format!("Could not read GitHub's answer: {err}"),
        },
        Err(err) => return format!("Could not read GitHub's answer: {err}"),
    };
    let is_release = body["draft"].as_bool() != Some(true);
    let version = body["tag_name"].as_str().unwrap_or_default().trim_start_matches('v').to_string();
    let address = body["assets"]
        .as_array()
        .and_then(|assets| {
            let name = |a: &&serde_json::Value| a["name"].as_str().unwrap_or_default().to_ascii_lowercase();
            assets
                .iter()
                .find(|a| name(a) == "aurora.exe")
                .or_else(|| assets.iter().find(|a| name(a).ends_with(".exe") && !name(a).contains("setup")))
                .and_then(|a| a["browser_download_url"].as_str())
                .map(str::to_string)
        });
    let (address, current) = match (is_release, version.is_empty(), address) {
        (true, false, Some(address)) => (address, env!("CARGO_PKG_VERSION")),
        _ => return no_release(),
    };
    if !is_newer(&version, current) {
        return format!("Aurora {current} is up to date. GitHub's newest release is {version}.");
    }
    match install(&version, &address) {
        Ok(()) => format!("Aurora {version} downloaded. It replaces this copy and starts again when you close Aurora."),
        Err(err) => format!("Aurora {version} is available, but the update could not be prepared: {err}"),
    }
}

fn no_release() -> String {
    format!("No executable build has been published at github.com/{REPO} yet.")
}

fn status_text(err: ureq::Error) -> String {
    match err {
        ureq::Error::Status(404, _) => no_release(),
        ureq::Error::Status(code, _) if code == 403 || code == 429 => {
            "GitHub is rate-limiting update checks right now. Try again in a few minutes.".into()
        }
        ureq::Error::Status(code, _) => format!("GitHub answered with an error ({code})."),
        ureq::Error::Transport(_) => "Could not reach GitHub. Check the connection and try again.".into(),
    }
}

/// Whether `candidate` (for example "0.3.0") is newer than `current`.
fn is_newer(candidate: &str, current: &str) -> bool {
    let parse = |v: &str| -> Option<Vec<u64>> {
        let core = v.trim().split(['-', '+']).next().unwrap_or_default();
        let parts: Vec<u64> = core.split('.').map(|p| p.parse().ok()).collect::<Option<_>>()?;
        (!parts.is_empty()).then_some(parts)
    };
    let (Some(candidate), Some(current)) = (parse(candidate), parse(current)) else { return false };
    let len = candidate.len().max(current.len());
    for i in 0..len {
        let (a, b) = (candidate.get(i).copied().unwrap_or(0), current.get(i).copied().unwrap_or(0));
        if a != b {
            return a > b;
        }
    }
    false
}

/// Downloads the new build and sets up the restart.
///
/// An installed copy (under `%LOCALAPPDATA%\Programs\Aurora`) receives the
/// browser executable, which the script swaps in place; the rest of the
/// install and the shortcuts stay valid. A portable copy is updated the same
/// way, in its own folder. The `aurora-setup.exe` installer is never picked
/// automatically: replacing an installer would leave the old install behind.
fn install(version: &str, address: &str) -> Result<(), String> {
    #[cfg(not(windows))]
    {
        let _ = (version, address);
        return Err(format!("download it from github.com/{REPO} and replace Aurora by hand"));
    }
    #[cfg(windows)]
    {
        let dir = updates_dir();
        fs::create_dir_all(&dir).map_err(|e| format!("could not create {}: {e}", dir.display()))?;
        let downloaded = dir.join(format!("aurora-{version}.exe"));
        let response = agent().get(address).call().map_err(|e| format!("the download failed: {e}"))?;
        let mut reader = response.into_reader().take(400 * 1024 * 1024);
        let mut file = fs::File::create(&downloaded).map_err(|e| format!("could not save the update: {e}"))?;
        std::io::copy(&mut reader, &mut file).map_err(|e| format!("the download failed: {e}"))?;
        let size = file.metadata().map(|m| m.len()).unwrap_or(0);
        drop(file);
        if size < 64 * 1024 {
            let _ = fs::remove_file(&downloaded);
            return Err("the downloaded file looks incomplete".into());
        }

        // Listed in the downloads panel like any other file.
        {
            let lib = crate::library::shared();
            let mut lib = lib.borrow_mut();
            lib.download_started(downloaded.clone());
            lib.download_finished(Some(downloaded.clone()), true);
            lib.flush();
        }

        // The setup exe unpacks beside itself when run, so later updates look
        // for the new version next to this executable.
        let exe = std::env::current_exe().map_err(|e| format!("could not find Aurora's own file: {e}"))?;
        let install_dir = exe.parent().map(Path::to_path_buf).unwrap_or_else(|| dir.clone());
        let script = install_dir.join(SCRIPT_NAME);
        // A protected folder cannot hold the script; the new file is then left
        // in Updates for the user to move.
        if restart_script(&script, &downloaded, &exe).is_err() {
            return Err(format!(
                "the folder {} cannot be written to; the new version is in {}",
                install_dir.display(),
                downloaded.display()
            ));
        }
        spawn_script(&script);
        Ok(())
    }
}

/// Writes the script that swaps `downloaded` in for `exe` once Aurora exits.
#[cfg(windows)]
fn restart_script(script: &Path, downloaded: &Path, exe: &Path) -> std::io::Result<()> {
    let text = format!(
        "@echo off\n\
         rem Written by Aurora's update check. Waits for Aurora to close,\n\
         rem puts the new build in place, removes itself and starts Aurora.\n\
         setlocal\n\
         set \"TARGET={exe}\"\n\
         set \"NEW={downloaded}\"\n\
         set \"SELF=%~f0\"\n\
         for /L %%i in (1,1,90) do (\n\
         \x20 tasklist /FI \"IMAGENAME eq aurora.exe\" 2>nul | find /I \"aurora.exe\" >nul || goto swap\n\
         \x20 ping -n {POLL_SECONDS} 127.0.0.1 >nul 2>&1\n\
         )\n\
         goto failed\n\
         :swap\n\
         timeout /t 1 /nobreak >nul 2>&1\n\
         move /Y \"%NEW%\" \"%TARGET%\" >nul 2>&1\n\
         if errorlevel 1 goto failed\n\
         start \"\" \"%TARGET%\"\n\
         goto cleanup\n\
         :failed\n\
         start \"\" explorer /select,\"%NEW%\"\n\
         :cleanup\n\
         (goto) 2>nul & del \"%SELF%\"\n",
        exe = exe.display(),
        downloaded = downloaded.display(),
    );
    fs::write(script, text)
}

#[cfg(windows)]
fn spawn_script(script: &Path) {
    use std::os::windows::process::CommandExt as _;
    let _ = std::process::Command::new("cmd.exe")
        .arg("/c")
        .arg(script)
        .creation_flags(0x08000000) // CREATE_NO_WINDOW
        .spawn();
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn versions_are_ordered() {
        assert!(is_newer("0.3.0", "0.2.0"));
        assert!(is_newer("1.0", "0.9.9"));
        assert!(is_newer("0.2.1", "0.2"));
        assert!(!is_newer("0.2.0", "0.2.0"));
        assert!(!is_newer("0.1.9", "0.2.0"));
        assert!(!is_newer("nightly", "0.2.0"));
    }
}
