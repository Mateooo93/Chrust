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
- Ad and tracker blocking, on by default: requests are checked against Brave's
  filter engine and stopped before they leave the machine, including YouTube's
  in-stream ads. See [Ad blocking](#ad-blocking).
- Google account sign-in from Settings › Privacy; sites that offer "Sign in
  with Google" then use the same session.
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
git clone https://github.com/Mateooo93/chrust-port
cd chrust-port
cargo build --release
.\target\release\chrust-port.exe
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
.\target\release\chrust-port.exe
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

### Proxy and VPN

Settings › Privacy › **VPN** sends every page and every download through a
VPN or proxy. Chrust does not run a VPN client of its own, so the address is
the tunnel one is already listening on: pick your app from the preset list
(Mullvad, Proton VPN, NordVPN, Surfshark and the rest use
`socks5://127.0.0.1:1080`, `9050` or `8080`), or type `socks5://host:port` or
`http://host:port`. Chrust only accepts an address with a scheme and a port,
so a typo cannot turn into a tunnel to nothing. A change applies on restart,
which the card offers; your tabs are reopened afterwards.

`CHRUST_PROXY`, set before starting Chrust, overrides the saved address for
that run. Either way the page engine, extension downloads, update checks and
site icons all go through it.

```powershell
$env:CHRUST_PROXY = "socks5://127.0.0.1:9150"   # or http://127.0.0.1:8080
.\target\release\chrust-port.exe
```

## Ad blocking

Ad blocking is on by default and works without an extension. Every request a
page makes goes through Brave's `adblock` engine (the one behind Brave's
shields, matching the same Adblock Plus syntax as uBlock Origin); a matching
request is answered with an empty 403 before it can load.

The rules are built into `chrust.exe` so a fresh install blocks on its first
page and keeps blocking offline:

- `assets/filters/chrust-adhosts.txt` – a host blocklist, generated from
  [pgl.yoyo.org](https://pgl.yoyo.org/adservers/).
- `assets/filters/chrust-curated.txt` – first-party ad and tracking hosts a
  host-only list misses, plus the rules YouTube needs for its in-stream ads
  (Google's ad delivery hosts and `youtube.com/api/stats/ads`,
  `youtube.com/pagead/`, `youtube.com/ptracking`).

Settings › Privacy can add the daily **EasyList** and **EasyPrivacy** lists on
top of those rules. They are downloaded in the background when Chrust starts;
a failed download leaves the built-in rules in place.

The lists are generated by `node tools/update-filters/build.js`; regenerate
them before a release. Adding a rule by hand to `chrust-curated.txt` is fine,
but it is overwritten by the next regeneration because the curated list lives
in `build.js`.

## Google accounts

Settings › Privacy › **Sign in** opens Google's own sign-in page. Chrust does
not create an account or keep a token of its own: signing in leaves Google's
cookies in the WebView2 profile, exactly as browsing to the site would, and
every site that offers "Sign in with Google" uses that session. Clearing
browsing data signs out everywhere, Google included.

The sign-in pages are listed in `src/signin.rs`. Ad blocking skips them, so a
rule can never break a login, and a popup that asks for a size is opened by
WebView2 itself, which keeps `window.opener` for the page that asked.

## Where Chrust keeps things

Everything lives in `%LOCALAPPDATA%\Chrust` (kept under the earlier name, so data survives): the WebView2 profile (cookies,
logins, site data), bookmarks, history, downloads, settings, session, cached
site icons and downloaded updates. Uninstalling leaves that folder alone, so
your data is still there if you reinstall.

## Licence

See the repository for licence details.
