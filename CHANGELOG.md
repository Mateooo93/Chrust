# Changes

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
