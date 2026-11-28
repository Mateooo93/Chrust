# Refreshing the filter lists

`assets/filters` is compiled into `chrust.exe` (with `include_str!` in
`src/adblock.rs`), so a release blocks ads from its very first page and keeps
working offline.

## Regenerate the lists

From the repository root:

```powershell
node tools/update-filters/build.js
git add assets/filters
```

This rewrites both files from their upstream sources and prints how many rules
each one has. Run it before tagging a release; nothing else needs to change,
the new rules become part of the build.

## What the two files are

| File | Source | Rules |
| --- | --- | --- |
| `chrust-adhosts.txt` | [pgl.yoyo.org](https://pgl.yoyo.org/adservers/) host blocklist, rewritten as `||host^` | ~3,500 |
| `chrust-curated.txt` | The `CURATED` array in `build.js` | ~70 |

The curated list exists for two reasons:

1. **First-party ad hosts.** Host-only lists skip anything on a site's own
   domain, and plenty of ad delivery now happens there.
2. **YouTube.** Its in-stream ads are served from `pubads.g.doubleclick.net`
   and requested by the player itself from `/api/stats/ads`, `/pagead/` and
   `/ptracking`. Blocking those endpoints, and the Google ad hosts, is what
   removes YouTube ads; the player, the video stream and its APIs are left
   alone on purpose.

To add a rule, add it to `CURATED` in `build.js` and rerun the script. Editing
`chrust-curated.txt` by hand works until the next regeneration overwrites it.

## Syntax

Rules are ordinary Adblock Plus syntax, checked by the `adblock` engine the
same way uBlock Origin and Brave read it: `||host^`, `|https://…|`, `$script`,
`$image`, `$xmlhttprequest`, `$third-party`, `$domain=a|b`, and `@@` for an
exception. Cosmetic rules (`##selector`, scriptlets) parse but have no effect
in Chrust, which cannot inject a stylesheet into WebView2 pages.

## Testing a rule before release

`cargo test adblock` checks the shipped lists against real requests: Google's
ad delivery, YouTube's ad endpoints and the lower-level host list must block,
while the YouTube player, a video stream, a thumbnail and the sign-in pages
must not. Add a case there when you add a rule that could break a site.
