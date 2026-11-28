// Builds the filter lists embedded in Chrust (assets/filters), so a release
// does not have to download rules before it can block an ad.
//
// Run from the repository root:
//     node tools/update-filters/build.js
//
// The output is deterministic apart from the "Generated" date line. Upstream
// lists are only used to fill the files; the rules Chrust ships are plain
// Adblock Plus syntax so they can be read by eye before release.

const fs = require("node:fs/promises");
const path = require("node:path");

const OUT = path.join(__dirname, "..", "..", "assets", "filters");

async function get(url) {
  const response = await fetch(url, { headers: { "User-Agent": "Chrust filter build" } });
  if (!response.ok) throw new Error(`${response.status} for ${url}`);
  return response.text();
}

// Turns a host-only blocklist (yoyo.org's "plain" format, or a hosts file)
// into Adblock Plus network rules: `||host^`.
function hostsToRules(text) {
  const out = [];
  for (const raw of text.split(/\r?\n/)) {
    const line = raw.trim();
    if (!line || line.startsWith("#") || line.startsWith("!")) continue;
    const parts = line.split(/\s+/);
    let host = parts[0];
    if (parts.length > 1 && /^(0\.0\.0\.0|127\.0\.0\.1|::1?)$/.test(parts[0])) host = parts[1];
    host = host.toLowerCase().replace(/\.$/, "");
    if (host.includes("/") || host.includes(":")) continue; // URLs and IPv6
    if (!/^[a-z0-9.-]+\.[a-z]{2,}$/.test(host)) continue;
    if (host === "localhost" || host.endsWith(".local")) continue;
    out.push(`||${host}^`);
  }
  return out;
}

// First-party ad and tracking hosts that a plain ad-server list misses, plus
// the rules YouTube needs: its in-stream ads come from Google's own ad
// delivery hosts and from `/api/stats/ads` on youtube.com, neither of which a
// host-only list can block without breaking playback.
const CURATED = [
  "||doubleclick.net^",
  "||googlesyndication.com^",
  "||googleadservices.com^",
  "||googletagservices.com^",
  "||googletagmanager.com^$third-party",
  "||google-analytics.com^",
  "||analytics.google.com^",
  "||adservice.google.com^",
  "||pagead2.googlesyndication.com^",
  "||partner.googleadservices.com^",
  "||pubads.g.doubleclick.net^",
  "||securepubads.g.doubleclick.net^",
  "||googleads.g.doubleclick.net^",
  "||static.doubleclick.net^",
  "||stats.g.doubleclick.net^",
  "||cm.g.doubleclick.net^",
  "||ad.doubleclick.net^",
  "||td.doubleclick.net^",
  "||imasdk.googleapis.com^",
  "||ssl.google-analytics.com^",
  "||www.google-analytics.com^",
  "||youtube.com/api/stats/ads$xmlhttprequest,domain=youtube.com|youtube-nocookie.com",
  "||youtube.com/get_midroll_info$xmlhttprequest,domain=youtube.com",
  "||youtube.com/pagead/$domain=youtube.com",
  "||youtube.com/ptracking$domain=youtube.com",
  "||aaxads.com^",
  "||ads-twitter.com^",
  "||ads.linkedin.com^",
  "||ads.tiktok.com^",
  "||analytics.tiktok.com^",
  "||connect.facebook.net^$third-party",
  "||an.facebook.com^",
  "||amazon-adsystem.com^",
  "||s.amazon-adsystem.com^",
  "||adnxs.com^",
  "||rubiconproject.com^",
  "||pubmatic.com^",
  "||openx.net^",
  "||criteo.com^",
  "||criteo.net^",
  "||taboola.com^",
  "||outbrain.com^",
  "||scorecardresearch.com^",
  "||quantserve.com^",
  "||moatads.com^",
  "||casalemedia.com^",
  "||33across.com^",
  "||sharethrough.com^",
  "||teads.tv^",
  "||smartadserver.com^",
  "||gumgum.com^",
  "||adsrvr.org^",
  "||serving-sys.com^",
  "||mathtag.com^",
  "||bidswitch.net^",
  "||tremorhub.com^",
  "||undertone.com^",
  "||2mdn.net^",
  "||innovid.com^",
  "||springserve.com^",
  "||spotxchange.com^",
  "||spotx.tv^",
  "||trustx.org^",
  "||zemanta.com^",
  "||adform.net^",
  "||flashtalking.com^",
  "||sizmek.com^",
  "||adcolony.com^",
  "||applovin.com^",
  "||unityads.unity3d.com^",
  "||vungle.com^",
];

function header(title, upstream) {
  return [
    `! Title: ${title}`,
    `! Upstream: ${upstream}`,
    `! Generated: ${new Date().toISOString().slice(0, 10)}`,
    "! Regenerate with: node tools/update-filters/build.js",
  ].join("\n");
}

async function main() {
  const yoyo = await get(
    "https://pgl.yoyo.org/adservers/serverlist.php?hostformat=plain&showintro=0&mimetype=plaintext&hidewelcomemessage=1",
  );
  const hosts = hostsToRules(yoyo);
  await fs.mkdir(OUT, { recursive: true });
  await fs.writeFile(
    path.join(OUT, "chrust-adhosts.txt"),
    header("Chrust ad hosts", "https://pgl.yoyo.org/adservers/") + "\n" + hosts.join("\n") + "\n",
  );
  await fs.writeFile(
    path.join(OUT, "chrust-curated.txt"),
    header("Chrust curated ad and YouTube rules", "maintained in tools/update-filters/build.js") +
      "\n" + CURATED.join("\n") + "\n",
  );
  console.log(`chrust-adhosts.txt: ${hosts.length} rules`);
  console.log(`chrust-curated.txt: ${CURATED.length} rules`);
}

main().catch((err) => {
  console.error(err);
  process.exit(1);
});
