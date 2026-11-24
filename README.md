# Chrust

A fast, clean web browser for Windows. The interface is built in Rust with GPUI;
pages are rendered by Microsoft's Edge WebView2 engine, which ships with Windows,
so Chrust installs as a single small executable.

## Install

Download `chrust-setup.exe` from the [latest release](../../releases/latest) and
run it. It installs for your account only, adds a Start menu entry and an entry
in Apps and Features, and needs no administrator rights. Running it again
upgrades an existing install in place.

Prefer a portable copy? Download `chrust.exe` and run it from any folder.

Newer versions can also be installed from inside the browser: **Menu > Check for
updates** or **Settings > About > Check for updates**. Chrust compares itself with
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
- Chrust's own pages (new tab, settings) share the browser's theme and are
  updates live: a bookmark edit shows on the open new tab page immediately.

## Build

Requirements: Windows 10 or 11 with the WebView2 runtime (preinstalled on both),
Rust 1.85 or newer, and the MSVC build tools.

```powershell
git clone https://github.com/Mateooo93/aurora
cd chrust
cargo build --release
.\target\release\aurora.exe
```

The first build takes a while: the release profile is optimized heavily (full
LTO, one codegen unit) for the fastest runtime.

### Measuring start-up

Chrust writes how long start-up took to `chrust.log` (`window built: 120ms`,
`first frame: 180ms`, `first page ready: 460ms`). To time a build without a
stopwatch, ask it to measure itself and close:

```powershell
$env:CHRUST_DATA_DIR = "$env:TEMP\chrust-measure"   # never the profile you use
$env:CHRUST_SELFTEST = "4000"                       # close after four seconds
.\target\release\chrust.exe
Get-Content "$env:TEMP\chrust-measure\chrust.log"
```

`CHRUST_DATA_DIR` also makes a portable copy: everything Chrust keeps (profile,
bookmarks, history, settings, session) moves to that folder.

### Faster rebuilds

This machine builds with `rust-lld` and a warm `sccache`, both set up in
`~/.cargo/config.toml`:

```toml
[build]
rustc-wrapper = "rustc-wrap"      # hands most crates to sccache

[target.x86_64-pc-windows-msvc]
linker = "lld-link"
```

`rustc-wrap` (source in `tools/rustc-wrap`, installed as
`~/.cargo/bin/rustc-wrap.exe`) exists because Windows cannot give a process a
command line longer than 32 KB. The biggest crates,
`gpui-component` and Chrust itself, need more than that, and neither cargo nor
sccache copes once a wrapper is in front: sccache expands the argument list
and then fails with `failed to spawn ... (exit code: 0xfffffffe)`. The wrapper
sends short command lines to sccache and long ones straight to rustc; `sccache
-s` shows the hit rate.

### Logo

Chrust draws a built-in mark. To use your own, save a square image as
`assets/chrust/chrust-64.png`. Cargo does not notice the image changing, so
run `cargo clean -p chrust` after replacing it.

## Releases

Pushing a tag like `v0.4.1` builds `chrust.exe` and `chrust-setup.exe` on GitHub
Actions and publishes them as a release (`.github/workflows/release.yml`).
"Check for updates" reads that release. Bump `version` in `Cargo.toml` with the
tag so the check recognises the build as newer.

### Proxy

Set `AURORA_PROXY` before starting Chrust to send everything through a proxy:
the page engine, extension downloads, update checks and site icons.

```powershell
$env:AURORA_PROXY = "socks5://127.0.0.1:9150"   # or http://127.0.0.1:8080
.	argetelease\chrust.exe
```

## Where Chrust keeps things

Everything lives in `%LOCALAPPDATA%\Chrust` (kept under the earlier name, so data survives): the WebView2 profile (cookies,
logins, site data), bookmarks, history, downloads, settings, session, cached
site icons and downloaded updates. Uninstalling leaves that folder alone, so
your data is still there if you reinstall.

## Licence

See the repository for licence details.
