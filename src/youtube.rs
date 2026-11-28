//! YouTube video ads.
//!
//! A network rule cannot stop a video ad: the ad and the video are served by
//! the same `googlevideo.com` hosts, so there is no address to block, and
//! blocking `youtube.com` would take the player with it. What works is the
//! approach uBlock Origin takes: run a script inside YouTube that looks into
//! the player data and announces the end of every ad break, so the player
//! skips it.
//!
//! The hook has to be installed before YouTube's own scripts parse the
//! response, which is what an early initialization script in `tab` gives us.
//! Everything is best-effort: if YouTube changes its data layout the script
//! does nothing, and the page plays exactly as it would without it. Nothing
//! here runs outside YouTube.
//!
//! Only the ad machinery is touched: the script does not read or send
//! anything about the video or the viewer.

/// Whether `url` is a page this script belongs on, used to decide about a page
/// before it is built. The injected script checks the address again for every
/// later navigation (see `tab`), which is what actually gates it.
#[cfg_attr(not(test), allow(dead_code))]
pub fn applies(url: &str) -> bool {
    let rest = match url.strip_prefix("https://").or_else(|| url.strip_prefix("http://")) {
        Some(rest) => rest,
        None => return false,
    };
    let host = rest
        .split(['/', '?', '#'])
        .next()
        .unwrap_or_default()
        .rsplit('@')
        .next()
        .unwrap_or_default()
        .split(':')
        .next()
        .unwrap_or_default()
        .to_ascii_lowercase();
    host == "youtube.com"
        || host.ends_with(".youtube.com")
        || host == "youtube-nocookie.com"
        || host.ends_with(".youtube-nocookie.com")
}

/// The script injected into YouTube pages. It is plain JavaScript with no
/// template placeholders, so one copy serves every page.
pub const SCRIPT: &str = r#"
(() => {
  if (window.__chrustportYt) return;
  window.__chrustportYt = true;

  const AD_TYPES = ["ad", "ads", "doubleclick", "promo", "advertisement"];

  // The ad markers YouTube leaves in the progress bar and the "Skip" /
  // "Ad" overlay. They are only needed for the ad itself.
  const css = document.createElement("style");
  css.textContent = `
    .ad-showing .video-ads, .ad-showing .ytp-ad-overlay-container,
    .ad-showing .ytp-ad-text-overlay, .ytp-ad-module,
    .ytp-ad-survey, .ytp-ad-player-overlay, .ytp-ad-progress,
    #masthead-ad, ytd-ad-slot-renderer, ytd-in-feed-ad-layout-renderer,
    ytd-promoted-sparkles-web-renderer, ytd-display-ad-renderer,
    ytd-banner-promo-renderer, ytd-statement-banner-renderer,
    #player-ads, .ytd-merch-shelf-renderer { display: none !important; }
  `;
  (document.head || document.documentElement).appendChild(css);

  // Ask the player to stop the current ad rather than waiting for it. The
  // player object is the one YouTube keeps on the page for the <video>.
  const stopAd = () => {
    const player = document.getElementById("movie_player");
    if (!player) return false;
    try {
      if (typeof player.getAdState === "function" && player.getAdState() > 0) {
        if (typeof player.skipAd === "function") player.skipAd();
        const video = player.querySelector("video");
        if (video) {
          // Jump to the end of the ad; YouTube then moves on to the video.
          video.currentTime = Math.max(video.currentTime, (video.duration || 0) - 0.1);
        }
        return true;
      }
    } catch (_) {}
    return false;
  };

  // The truth about what is playing lives in the player data YouTube parses
  // before it starts. Wrapping the JSON parser lets the script see the ad
  // lists before the player does and announce the end of each break.
  const originalParse = JSON.parse;
  JSON.parse = function (text, reviver) {
    const out = originalParse.call(this, text, reviver);
    try { scan(out); } catch (_) {}
    return out;
  };

  const seen = new WeakSet();
  function scan(node) {
    if (!node || typeof node !== "object" || seen.has(node)) return;
    seen.add(node);
    if (Array.isArray(node)) { for (const v of node) scan(v); return; }
    const pv = node.playerVars;
    if (pv && typeof pv === "object") {
      const isAd = AD_TYPES.some((k) => pv[k] !== undefined && pv[k] !== null && pv[k] !== "")
        || (typeof pv.adformat === "string" && pv.adformat)
        || (Array.isArray(pv.ad_break_params) && pv.ad_break_params.length);
      if (isAd) {
        // Tell the player the break is already over. YouTube treats a break
        // whose offset equals its duration as finished and moves on.
        const len = Number(pv.ad_len) || Number(pv.adlen) || 0;
        if (len) {
          pv.ad_type = "skip";
          pv.ad_len = 0;
          pv.ad_preroll = 0;
          pv.ad_skipable = 1;
          pv.ad_skippable = 1;
        }
        pv.adformat = "";
        pv.adformat_type = "";
        delete pv.ad_break_params;
        setTimeout(stopAd, 50);
      }
    }
    // YouTube also sends the ad list as `adPlacements` / `playerAds`.
    for (const key of ["adPlacements", "playerAds", "adSlots"]) {
      if (Array.isArray(node[key]) && node[key].length) {
        node[key] = [];
        setTimeout(stopAd, 50);
      }
    }
    for (const k in node) scan(node[k]);
  }

  // A last line of defence: if an ad starts anyway, end it as soon as the
  // player says there is one.
  setInterval(() => {
    const player = document.getElementById("movie_player");
    if (!player) return;
    let state = 0;
    try { state = typeof player.getAdState === "function" ? player.getAdState() : 0; } catch (_) {}
    if (state > 0 || player.classList.contains("ad-showing")) stopAd();
  }, 500);
})();
"#;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn only_runs_on_youtube() {
        assert!(applies("https://www.youtube.com/watch?v=abc"));
        assert!(applies("https://youtube.com/"));
        assert!(applies("https://m.youtube.com/watch?v=abc"));
        assert!(applies("https://www.youtube-nocookie.com/embed/abc"));
        assert!(!applies("https://www.youtube.com.evil.example/watch?v=abc"));
        assert!(!applies("https://evilyoutube.com/"));
        assert!(!applies("https://example.com/?u=https://youtube.com/"));
        assert!(!applies("about:blank"));
    }

    #[test]
    fn the_script_is_self_contained() {
        // One copy is injected on every page, so it must not contain a
        // template placeholder by accident.
        assert!(!SCRIPT.contains("{{"));
        assert!(!SCRIPT.contains("}}"));
        assert!(SCRIPT.contains("__chrustportYt"));
    }
}
