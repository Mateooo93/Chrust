# Aurora

A fast, clean web browser for Windows. The interface is built in Rust with GPUI;
pages are rendered by Microsoft's Edge WebView2 engine, which ships with Windows,
so Aurora installs as a single small executable.

## Install

Download `aurora-setup.exe` from the [latest release](../../releases/latest) and
run it. It installs for your account only, adds a Start menu entry and an entry
in Apps and Features, and needs no administrator rights. Running it again
upgrades an existing install in place.

Prefer a portable copy? Download `aurora.exe` and run it from any folder.

Newer versions can also be installed from inside the browser: **Menu > Check for
updates** or **Settings > About > Check for updates**. Aurora compares itself with
the newest release here, downloads the executable, and swaps it in when you close
the browser.

## Features

- Tabs with loading indicators that end when a page fails, session restore,
  and private windows that leave no history.
- Chromium-style keyboard shortcuts (`Ctrl+T`, `Ctrl+W`, `Ctrl+Tab`, `Ctrl+L`,
  `Ctrl+F`, `F11`, and more); see Settings > Shortcuts for the full list.
- Bookmarks, history, downloads, find in page, zoom, DevTools, page source.
- Settings page with themes, search engines, animated new tab backgrounds and
  import of Chrome bookmarks.
- Aurora's own pages (new tab, settings) share the browser's theme and are
  updates live: a bookmark edit shows on the open new tab page immediately.

## Build

Requirements: Windows 10 or 11 with the WebView2 runtime (preinstalled on both),
Rust 1.85 or newer, and the MSVC build tools.

```powershell
git clone https://github.com/Mateooo93/aurora
cd aurora
cargo build --release
.\target\release\aurora.exe
```

The first build takes a while: the release profile is optimized heavily (full
LTO, one codegen unit) for the fastest runtime.

### Logo

Aurora draws a built-in mark. To use your own, save a square image as
`assets/logo.png` and build with `$env:AURORA_LOGO=1` set. Cargo does not notice
the image changing, so run `cargo clean -p aurora` after replacing it.

## Releases

Pushing a tag like `v0.3.1` builds `aurora.exe` and `aurora-setup.exe` on GitHub
Actions and publishes them as a release (`.github/workflows/release.yml`).
"Check for updates" reads that release. Bump `version` in `Cargo.toml` with the
tag so the check recognises the build as newer.

### Proxy

Set `AURORA_PROXY` before starting Aurora to send everything through a proxy:
the page engine, extension downloads, update checks and site icons.

```powershell
$env:AURORA_PROXY = "socks5://127.0.0.1:9150"   # or http://127.0.0.1:8080
.	argeteleaseurora.exe
```

## Where Aurora keeps things

Everything lives in `%LOCALAPPDATA%\Aurora`: the WebView2 profile (cookies,
logins, site data), bookmarks, history, downloads, settings, session, cached
site icons and downloaded updates. Uninstalling leaves that folder alone, so
your data is still there if you reinstall.

## Licence

See the repository for licence details.
