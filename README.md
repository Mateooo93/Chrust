<div align="center">

<img src="assets/chrust/chrust-256.png" width="120" alt="Chrust">

# Chrust

**A browser for Windows that opens faster than you can move your hand away from the click.**
The window, the tabs and every pixel of the interface are drawn by Rust + GPUI;
pages are rendered by the Edge WebView2 engine that already ships with Windows,
so Chrust installs as a single small executable. No Chromium of its own to ship,
no bundle of extensions to load, ads blocked before they leave the machine.

</div>

## Why it is fast

The chrome is not a web page. It is a native GPUI surface, so a tab switch, a
keystroke or a menu costs nothing that a document tree would cost, and the
browser window is on screen before the first page has even asked for its
network connection. Nothing about the interface waits for the engine.

| What | Measured on this machine |
| --- | --- |
| Window on screen (first frame) | **195–267 ms** from launch, engine start included |
| 8-tab session restored, first page readable | **770–930 ms** |
| 8-tab session restored, all 8 pages ready | **1.6–2.0 s** (cold profile: 1.9 s) |
| New tab, page prepared ahead of time | **instant** — the tab appears, the page is already there |
| New tab, page not prepared (slower path) | **~200 ms** |
| Interface memory | **~85 MB private, and flat**: the same with one tab or eight |
| Page engine for those 8 tabs | WebView2's own processes, ~780 MB working set |

Numbers come from Chrust's own log (`chrust.log`) and from `Get-Process` on the
same runs, on Windows 11 with an Intel UHD GPU. They are the reason the code
looks the way it does:

- **The next tab is started early.** Once the window settles, Chrust opens one
  hidden page and keeps it ready. `Ctrl+T` inserts that page and changes its
  address instead of building a new one, which is the difference between a tab
  that appears at once and one that appears after the engine wakes up.
- **A restored session is loaded in parallel, not one tab after another**, and
  each page is put back where it was so the strip does not shuffle.
- **Hidden tabs are told to give memory back** and the visible tab to recompose,
  but only when that actually changes, so a tab switch is one call into the
  engine instead of one per tab.
- **No spinner outlives its page.** Each load is watched from a timer; if the
  engine never reports the load finished, the page itself is asked whether its
  document is complete, and the spinner ends when it says yes.
- **Settings, history and the session are written on a coalescing timer**, never
  in the middle of a frame.
- **The release profile is built for runtime speed**: fat LTO, one codegen unit,
  `panic = "abort"`, no debug symbols.

### About memory

The interface itself is about 85 MB of private memory, and it does not grow with
the number of tabs — measured at 87 MB with a single new tab and 83 MB with
eight loaded pages. The pages live in WebView2's processes and cost what the
pages cost; a browser that renders the modern web cannot make them smaller, and
any claim otherwise would be a lie. What Chrust does control, it keeps small:
nothing is bundled that Windows already has — no rendering engine, no runtime,
no copy of the WebView2 libraries — and the whole browser is one
`chrust-port.exe` of about 25 MB on disk.

## Features

- Tabs, private windows, session restore, and a loading indicator that always
  ends.
- **Ad and tracker blocking, on by default**: requests are checked against
  Brave's filter engine and stopped before they leave the machine, including
  YouTube's in-stream ads. See [Ad blocking](#ad-blocking).
- **Google account sign-in** from Settings › Privacy; sites that offer "Sign in
  with Google" then use the same session.
- **VPN or proxy** for everything the browser does, from a preset list of
  providers or a typed address. See [Proxy and VPN](#proxy-and-vpn).
- **Registers itself as a browser** so Windows 11 offers it in Default apps.
- Chromium-style shortcuts (`Ctrl+T`, `Ctrl+W`, `Ctrl+Tab`, `Ctrl+L`, `Ctrl+F`,
  `F11`, and more); Settings > Shortcuts lists them all.
- Bookmarks bar, history, downloads, find in page, zoom, DevTools, page source.
- Themes for the whole browser, search engines, an animated new tab background,
  top sites, and Chrome bookmark import.
- Chrust's own pages (new tab, settings) use the browser theme and update live: a
  bookmark edit shows on the open new tab page immediately.

## Install

Download `chrust-setup.exe` from the [latest release](../../releases/latest) and
run it. It installs for your account only, adds a Start menu entry and an entry
in Apps and Features, and needs no administrator rights. Running it again
upgrades an existing install in place.

Prefer a portable copy? Download `chrust-port.exe` and run it from any folder.

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

## Ad blocking

Ad blocking is on by default and works without an extension. Every request a
page makes goes through Brave's `adblock` engine (the one behind Brave's
shields, matching the same Adblock Plus syntax as uBlock Origin); a matching
request is answered with an empty 403 before it can load.

The rules are built into the executable, so a fresh install blocks on its first
page and keeps blocking offline:

- `assets/filters/chrust-adhosts.txt` – a host blocklist, generated from
  [pgl.yoyo.org](https://pgl.yoyo.org/adservers/).
- `assets/filters/chrust-curated.txt` – first-party ad and tracking hosts a
  host-only list misses, plus the rules YouTube needs for its in-stream ads
  (Google's ad delivery hosts and `youtube.com/api/stats/ads`,
  `youtube.com/pagead/`, `youtube.com/ptracking`).

Settings › Privacy can add the daily **EasyList** and **EasyPrivacy** lists on
top of those rules. They are downloaded in the background when Chrust starts; a
failed download leaves the built-in rules in place.

The lists are generated by `node tools/update-filters/build.js`; regenerate them
before a release. Adding a rule by hand to `chrust-curated.txt` is fine, but it
is overwritten by the next regeneration because the curated list lives in
`build.js`.

## Google accounts

Settings › Privacy › **Sign in** opens Google's own sign-in page. Chrust does not
create an account or keep a token of its own: signing in leaves Google's cookies
in the WebView2 profile, exactly as browsing to the site would, and every site
that offers "Sign in with Google" uses that session. Clearing browsing data
signs out everywhere, Google included.

The sign-in pages are listed in `src/signin.rs`. Ad blocking skips them, so a
rule can never break a login, and a popup that asks for a size is opened by
WebView2 itself, which keeps `window.opener` for the page that asked.

## Default browser

Windows 11 will not let a program make itself the default; only the user can
choose, in **Settings › Apps › Default apps**. What Chrust can do is register
itself as a browser so it appears there:

```powershell
.\target\release\chrust-port.exe --register-browser    # add to "Default apps"
.\target\release\chrust-port.exe --unregister-browser  # remove again
```

Settings › About has a **Register as a browser** button that runs the first
command and a **Default apps** button that opens the Settings page. The
installer's uninstall script runs the second.

Registration is per user (`HKCU`) and touches only Chrust's own keys:
`Software\Clients\StartMenuInternet\ChrustPort` with its `Capabilities`,
`Software\RegisteredApplications`, and the `ChrustPortHTM` ProgId that `http`
and `https` links are opened with. Chrust opens a link by starting normally with
the address as its argument, the same as
`chrust-port.exe https://example.com`.

A link clicked while Chrust is already open starts a second window rather than a
tab in the first, because the new process cannot join the running one; closing
the extra window keeps both on the profile they share.

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

Pushing a tag like `v0.5.0` builds `chrust-port.exe` and `chrust-setup.exe` on
GitHub Actions and publishes them as a release
(`.github/workflows/release.yml`). "Check for updates" reads that release. Bump
`version` in `Cargo.toml` with the tag so the check recognises the build as
newer.

### Proxy and VPN

Settings › Privacy › **VPN** sends every page and every download through a VPN
or proxy. Chrust does not run a VPN client of its own, so the address is the
tunnel one is already listening on: pick your app from the preset list (Mullvad,
Proton VPN, NordVPN, Surfshark and the rest use `socks5://127.0.0.1:1080`,
`9050` or `8080`), or type `socks5://host:port` or `http://host:port`. Chrust
only accepts an address with a scheme and a port, so a typo cannot turn into a
tunnel to nothing. A change applies on restart, which the card offers; your tabs
are reopened afterwards.

`CHRUST_PROXY`, set before starting Chrust, overrides the saved address for that
run. Either way the page engine, extension downloads, update checks and site
icons all go through it.

```powershell
$env:CHRUST_PROXY = "socks5://127.0.0.1:9150"   # or http://127.0.0.1:8080
.\target\release\chrust-port.exe
```

## Where Chrust keeps things

Everything lives in `%LOCALAPPDATA%\Chrust` (kept under the earlier name, so data
survives): the WebView2 profile (cookies, logins, site data), bookmarks, history,
downloads, settings, session, cached site icons and downloaded updates.
Uninstalling leaves that folder alone, so your data is still there if you
reinstall.

## Licence

See the repository for licence details.
