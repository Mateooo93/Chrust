<div align="center">

<img src="assets/chrust/chrust-256.png" width="120" alt="Chrust">

# Chrust

**A browser for Windows that opens faster than you can move your hand away from the click.**
The window, the tabs and every pixel of the interface are drawn by Rust + GPUI;
pages are rendered by the Edge WebView2 engine that already ships with Windows.

</div>

## Why it is fast

The chrome is not a web page. It is a native GPUI surface, so a tab switch, a
keystroke or a menu costs nothing that a document tree would cost, and the
browser window is on screen before the first page has even asked for its
network connection. Nothing about the interface waits for the engine.

| What | Measured on this machine |
| --- | --- |
| Window on screen (first frame) | **92–142 ms** from launch |
| Session restored, 8 tabs, all pages loaded | **~1.0 s** (slowest page ready at 1.0–1.3 s) |
| New tab, page prepared ahead of time | **instant** — the tab appears, the page is already there |
| New tab, page not prepared (slower path) | **~200 ms** |
| Interface resident memory | **{{SHELL_MEM}}** |
| Page engine processes for those 8 tabs | **{{ENGINE_MEM}}** |

Numbers come from Chrust's own log (`chrust.log`) and from `Get-Process` on the
same run, on Windows 11 with a warm WebView2. They are the reason the code looks
the way it does:

- **The next tab is started early.** Once the window settles, Chrust opens one
  hidden page and keeps it ready. `Ctrl+T` inserts that page and changes its
  address instead of building a new one, which is the difference between a tab
  that appears at once and one that appears after the engine wakes up.
- **A restored session is loaded in parallel, not one tab after another**, and
  each page is put back where it was so the strip does not shuffle.
- **Hidden tabs are told to give memory back** and the visible tab is told to
  recompose, but only when that actually changes, so a tab switch is one call
  into the engine instead of one per tab.
- **Settings, history and the session are written on a coalescing timer**, never
  in the middle of a frame.
- **The release profile is built for runtime speed**: fat LTO, one codegen unit,
  `panic = "abort"`, no debug symbols.

### About memory

Chrust's own interface is a few megabytes — the number above is the whole
browser window, tabs, bookmarks bar and its cache of site icons. The pages
themselves live in WebView2's processes and cost what the pages cost; a browser
that renders the modern web cannot make them smaller, and any claim otherwise
would be a lie. What Chrust does control, it keeps small: nothing is bundled
that Windows already has — no rendering engine, no runtime, no copy of the WebView2
libraries — so the whole browser is one `chrust.exe` of about 25 MB on disk.

## Features

- Tabs, private windows, session restore, a loading indicator that always ends.
- Chromium-style shortcuts (`Ctrl+T`, `Ctrl+W`, `Ctrl+Tab`, `Ctrl+L`, `Ctrl+F`,
  `F11`, and more); Settings > Shortcuts lists them all.
- Bookmarks bar, history, downloads, find in page, zoom, DevTools, page source.
- Themes for the whole browser, search engines, an animated new tab background,
  top sites, and Chrome bookmark import.
- Chrust's own pages (new tab, settings) use the browser theme and update live: a
  bookmark edit shows on the open new tab page immediately.
- Optional proxy for everything the browser does: set `AURORA_PROXY`.

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

## Build

Requirements: Windows 10 or 11 with the WebView2 runtime (preinstalled on both),
Rust 1.85 or newer, and the MSVC build tools.

```powershell
git clone https://github.com/Mateooo93/Chrust
cd Chrust
cargo build --release
.\target\release\chrust.exe
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

## Icons

<img src="assets/chrust/chrust-256.png" width="72" align="right" alt="Ember mark">

`assets/chrust/chrust.ico` is the browser's own mark, the **ember** recolour of
the Chrome geometry: warm coal centre, ember orange top, amber lower left, deep
copper lower right. `assets/chrust/chrust.rc` puts it on the executable, the
installer copies it for shortcuts, and the new tab page shows it.

`assets/chrome/` holds the raw icon set taken out of an installed Chrome
(`chrome.ico`, plus 16, 32, 48 and 256 px PNGs) and the recolours made from it.
`ember-family.png` and `orange-options.png` are contact sheets of every variant.
The scripts read the icon groups straight out of a PE file, so they still work
after Chrome updates:

```powershell
python tools/extract-icons/extract_icons.py `
  "C:\Program Files\Google\Chrome\Application\chrome.exe" assets/chrome
python tools/extract-icons/recolour_logo.py `
  assets/chrome/chrome-256.png assets/chrome ember
```

`recolour_logo.py` hue-maps the segments while keeping their shading, gradients
and the white ring, and writes 16, 32, 48, 64, 128 and 256 px PNGs plus an
`.ico` that carries all of them. Palettes: `ember`, `ember-deep`, `ember-glow`,
`ember-rose`, `ember-copper`, `ember-light`, `amber`, `aurora`, `aurora-dark`.

## Faster rebuilds

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
command line longer than 32 KB. The biggest crates, `gpui-component` and Chrust
itself, need more than that, and neither cargo nor sccache copes once a wrapper
is in front: sccache expands the argument list and then fails with `failed to
spawn ... (exit code: 0xfffffffe)`. The wrapper sends short command lines to
sccache and long ones straight to rustc; `sccache -s` shows the hit rate.

## Releases

Pushing a tag like `v0.4.3` builds `chrust.exe` and `chrust-setup.exe` on GitHub
Actions and publishes them as a release (`.github/workflows/release.yml`).
"Check for updates" reads that release. Bump `version` in `Cargo.toml` with the
tag so the check recognises the build as newer.

### Proxy

Set `AURORA_PROXY` before starting Chrust to send everything through a proxy:
the page engine, extension downloads, update checks and site icons.

```powershell
$env:AURORA_PROXY = "socks5://127.0.0.1:9150"   # or http://127.0.0.1:8080
.\target\release\chrust.exe
```

## Where Chrust keeps things

Everything lives in `%LOCALAPPDATA%\Chrust` (kept under the earlier name, so data
survives): the WebView2 profile (cookies, logins, site data), bookmarks, history,
downloads, settings, session, cached site icons and downloaded updates.
Uninstalling leaves that folder alone, so your data is still there if you
reinstall.

## Licence

See the repository for licence details.
