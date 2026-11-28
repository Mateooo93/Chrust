# Changes

## 0.5.0 – ad blocking and Google accounts

- ChrustPort can register itself as a browser, so Windows offers it in
  Settings › Apps › Default apps. Windows will not let a program choose
  itself as the default, so registering adds it to the list and the user
  picks it there; Settings › About has buttons for both registering and
  opening that page. Registration is per user (`HKCU`) and covers
  `Software\Clients\StartMenuInternet\ChrustPort`, `RegisteredApplications`
  and a `ChrustPortHTM` ProgId for `http` and `https`; the installer's
  uninstall script unregisters it. `--register-browser` and
  `--unregister-browser` do the same from a command line. A link opens in a
  new Chrust window (`chrust-port.exe <url>`).

- This build is the `chrust-port` port: the executable is `chrust-port.exe` and
  its own pages, protocol and settings identifiers use `chrustport` so they
  cannot collide with an installed Chrust. Bookmarks, history and the WebView2
  profile are shared with Chrust, in `%LOCALAPPDATA%\Chrust`.

- Ads and trackers are blocked before they load. Requests pass through Brave's
  `adblock` filter engine (the one behind Brave's shields, so the same
  Adblock Plus syntax as uBlock Origin) and a match is answered with an empty
  403, exactly as a browser with its own network stack would. The rules ship
  inside the executable, so blocking works on the first start and offline:
  a 3,565-host blocklist plus a curated list that covers Google's ad
  delivery hosts and YouTube's own ad endpoints.
- YouTube ads are targeted directly: in-stream ads come from
  `pubads.g.doubleclick.net` and from `youtube.com/api/stats/ads`,
  `youtube.com/pagead/` and `youtube.com/ptracking`, none of which a
  host-only list blocks without breaking the player. The player, the video
  stream and the APIs it needs are explicitly left alone; the tests check
  both directions.
- Settings › Privacy has a switch for ad blocking, a switch to add the daily
  EasyList and EasyPrivacy lists on top of the built-in rules (downloaded in
  the background; a failed download keeps the working rules), and a status
  line. A change applies without a restart.
- Private windows always block, even when the setting is off, so a private
  tab never shows what the profile has seen.
- Extension scripts and page fetches are covered too: the filter applies to
  every request WebView2 makes, including those started by content scripts,
  service workers and subframes.
- Google sign-in: Settings › Privacy has a Google account button that opens
  Google's own sign-in page in a tab. The session stays in the browser's
  WebView2 profile, so "Sign in with Google" on any site uses it and
  Chrome-style clearing of site data signs out. Google's sign-in hosts are
  recognised (`src/signin.rs`) and ad blocking never touches them, so a login
  cannot be turned into a dead end by a rule; a popup that asks for a size
  (the account chooser) is left to the engine, which keeps `window.opener`.

## 0.4.3 – instant tabs

- A new tab no longer waits for the page engine: one page is started in the
  background a few seconds after a window settles (and again after each one
  is used), so Ctrl+T only has to show it and set the address. Measured on
  this machine, a new tab used to hold the window for about 200 ms while the
  engine started; it now appears at once. `CHRUST_NO_PREWARM=1` turns it off,
  and the spare page costs one hidden document.
- The spare page is never saved into the session and never shows in the tab
  strip: an eight-tab restore still comes back as exactly eight tabs, with
  the same active tab.

## 0.4.2 – settled

- Fixed: the spinner sometimes kept turning on a page that had been readable
  for a while. The load is now finished by the engine's own navigation
  result, and a load that reports nothing for 30 seconds is written off
  instead of spinning forever.
- Fixed: "Check for updates" never found a release. The repository moved
  from `aurora` to `Chrust`, and the old API path answers with a redirect
  that carries no release; the check now asks the current name and reports
  what GitHub says instead of "nothing has been published".
- The Linux build compiles again: the WebView2 error helpers are Windows-only
  code and are now kept out of the other platforms' builds.

## 0.4.1 – quicker

- One page-engine environment for the whole browser: the first page starts it
  and every tab after that joins it, instead of making the engine look itself up
  again for each page (which wry does, on the thread that draws the window).
- A restored window shows the page you were looking at straight away and brings
  the other tabs back as it draws, instead of holding the first frame until
  every page has started.
- Page events are no longer dropped when the window is busy: a moment of work
  used to make the browser lose a title or a loading state, and the first tab
  that arrived while the engine was starting could end the event loop for good.
- History and session writes now happen while the window is drawing, not from a
  timer that could fire in the middle of a frame (the usual reason opening a
  tab on a restored window ended in a crash).
- New tabs appear at once: the custom background and the cached site icons are
  served from memory rather than read from disk on the window thread, and the
  tab strip no longer re-parses and re-copies every address on every frame.
- Tab switching only tells the engine what actually changed, the window title is
  only set when it differs, and history search no longer lowercases the whole
  history on every frame.
- Fixed: a restored session kept its tab order and its active tab; every tab
  used to be inserted after the active one, shuffling the row on each start.
- Fixed: a finished update download closed the browser (the download is now
  recorded on the window thread).
- Fixed: the tab strip could act on the wrong tab if the list changed between
  drawing and clicking; a crash-reporting link is no longer opened in the wrong
  tab either.
- Fixed: opening a tab while the engine was busy reopened a blank page instead
  of the address that was asked for.
- Fixed: Back and Forward with nothing to go back to left the spinner running,
  and a load that failed now reports whether it rendered.
- Fixed: a site icon could be requested several times over while the first
  download was still running.
- Start-up timings are written to the log (`first frame: …ms`,
  `first page ready: …ms`). Set `CHRUST_SELFTEST=<milliseconds>` to have the
  browser measure itself and close, and `CHRUST_DATA_DIR` to keep a measurement
  away from your real profile.
- A crash now leaves a backtrace in `chrust.log` when `RUST_BACKTRACE=1` is
  set, so the next one names the code that was running, not just the line it
  died on.

## 0.4.0 – Chrust

- Renamed from Aurora to Chrust, with a new logo (window icon, tab strip,
  built-in pages, installer). Bookmarks, history and settings are kept: they
  live in the same data folder as before.
- Chrome Web Store extensions: install from a store link in Settings >
  Extensions, or from the store's own install button.
- A proxy can be set for the whole browser with `AURORA_PROXY`.

## 0.2.0 (unreleased)

- New loading indicator behaviour: failed and cancelled page loads now end the
  spinner, and Reload, Back and Forward show progress again.
- Faster with many tabs: history and session saves are coalesced, the favicon
  cache is read once, hidden tabs use less memory and CPU, and the window shows
  up before WebView2 starts.
- Built-in pages: cached site icons, a cacheable logo, and the new tab clock
  pauses while hidden.
- Bookmark edits update an open new tab page in place; settings survive a
  reload of the settings page.
- Check for updates (Settings > About) and a per-user installer.
