//! Built-in pages served through the `aurora` custom protocol.
//!
//! Each page gets its data as one JSON blob and builds its DOM with
//! `textContent`, so titles and URLs from bookmarks never become markup.

use serde_json::json;

use crate::library::Library;
use crate::settings::{self, BACKGROUNDS, SEARCH_ENGINES};
use crate::themes;

/// JSON that is safe inside a `<script>` element.
fn script_json(value: &serde_json::Value) -> String {
    value
        .to_string()
        .replace("</", "<\\/")
        .replace('\u{2028}', "\\u2028")
        .replace('\u{2029}', "\\u2029")
}

/// Styles shared by the built-in pages, including the new tab backgrounds
/// (the settings page uses them for previews).
const BASE_CSS: &str = r#"
*{box-sizing:border-box}
[hidden]{display:none!important}
html,body{margin:0;min-height:100%;background:var(--bg);color:var(--fg);
  font-family:"Segoe UI Variable Text","Segoe UI",system-ui,sans-serif;-webkit-font-smoothing:antialiased}
a{color:var(--accent)}
button,input,select{font:inherit;color:inherit}
::selection{background:color-mix(in srgb,var(--accent) 35%,transparent)}
.bg{position:fixed;inset:0;overflow:hidden;z-index:0;background:var(--bg)}
.bg i{position:absolute;border-radius:50%;filter:blur(90px);opacity:.55}
.bg-aurora i:nth-child(1){width:55vmax;height:55vmax;left:-12vmax;top:-18vmax;background:var(--accent);animation:drift1 26s ease-in-out infinite alternate}
.bg-aurora i:nth-child(2){width:45vmax;height:45vmax;right:-14vmax;top:-8vmax;background:var(--accent2);animation:drift2 32s ease-in-out infinite alternate}
.bg-aurora i:nth-child(3){width:40vmax;height:40vmax;left:25vmax;bottom:-26vmax;background:color-mix(in srgb,var(--accent) 50%,var(--accent2));animation:drift3 38s ease-in-out infinite alternate}
.bg-aurora::after{content:"";position:absolute;inset:0;background:radial-gradient(transparent 0,var(--bg) 85%);opacity:.55}
.bg-nebula{background:radial-gradient(ellipse at 20% 0%,color-mix(in srgb,var(--accent) 45%,transparent),transparent 60%),
  radial-gradient(ellipse at 90% 80%,color-mix(in srgb,var(--accent2) 40%,transparent),transparent 55%),#07070d}
.bg-nebula::before{content:"";position:absolute;inset:-50%;background:conic-gradient(from 0deg,transparent,color-mix(in srgb,var(--accent) 18%,transparent),transparent 30%);animation:spin 60s linear infinite}
.bg-nebula::after{content:"";position:absolute;inset:0;background-image:radial-gradient(1px 1px at 10% 20%,#fff9,transparent),radial-gradient(1px 1px at 70% 30%,#fff8,transparent),
  radial-gradient(1.5px 1.5px at 40% 70%,#fffa,transparent),radial-gradient(1px 1px at 85% 85%,#fff7,transparent),radial-gradient(1px 1px at 25% 90%,#fff6,transparent),
  radial-gradient(1px 1px at 55% 10%,#fff8,transparent);background-size:340px 340px;animation:twinkle 6s ease-in-out infinite alternate}
.bg-grid{background:radial-gradient(circle at 50% -20%,color-mix(in srgb,var(--accent) 30%,transparent),transparent 55%),
  radial-gradient(circle,color-mix(in srgb,var(--fg) 14%,transparent) 1px,transparent 1.5px) 0 0/22px 22px,var(--bg)}
.bg-sunset{background:linear-gradient(160deg,#2b1055 0%,#7597de 45%,#ff9a8b 75%,#ffd5a8 100%)}
.bg-ocean{background:linear-gradient(160deg,#0f2027 0%,#203a43 45%,#2c5364 75%,#3aa6b9 100%)}
.bg-forest{background:linear-gradient(160deg,#0b2b26 0%,#134e5e 45%,#2f7a58 75%,#71b280 100%)}
.bg-solid{background:var(--bg)}
.bg-custom{background-size:cover;background-position:center}
.bg-custom::after{content:"";position:absolute;inset:0;background:linear-gradient(transparent 40%,#0006)}
@keyframes drift1{to{transform:translate(18vmax,12vmax) scale(1.15)}}
@keyframes drift2{to{transform:translate(-16vmax,18vmax) scale(.9)}}
@keyframes drift3{to{transform:translate(-14vmax,-10vmax) scale(1.2)}}
@keyframes spin{to{transform:rotate(360deg)}}
@keyframes twinkle{to{opacity:.4}}
@keyframes rise{from{opacity:0;transform:translateY(14px)}to{opacity:1;transform:none}}
.still *, .still *::before, .still *::after{animation:none!important;transition:none!important}
.toast{position:fixed;left:50%;bottom:28px;transform:translate(-50%,20px);opacity:0;padding:10px 16px;border-radius:12px;
  background:var(--elevated);color:var(--fg);border:1px solid var(--border);box-shadow:0 10px 30px #0004;transition:.25s;pointer-events:none;font-size:14px}
.toast.show{opacity:1;transform:translate(-50%,0)}
"#;

pub fn new_tab(lib: &Library, private: bool) -> String {
    let s = &lib.settings;
    let engine = settings::search_engine(&s.search_engine);
    let bookmarks: Vec<_> = lib
        .bookmarks
        .iter()
        .filter(|b| b.url.starts_with("https://") || b.url.starts_with("http://"))
        .take(6)
        .map(|b| json!({ "title": b.title, "url": b.url }))
        .collect();
    let custom_bg = if s.background_url.is_empty() {
        "http://aurora.newtab/background".to_string()
    } else {
        s.background_url.clone()
    };
    let data = json!({
        "private": private,
        "background": s.background,
        "customBackground": custom_bg,
        "clock": s.show_clock,
        "icons": s.site_icons,
        "animate": s.smooth_animations,
        "engine": { "name": engine.name, "url": engine.url },
        "bookmarks": bookmarks,
        // The logo address changes with each build, so the image can be cached.
        "logo": crate::browser::LOGO_PATH,
        "mirror": true,
    });

    format!(
        r##"<!doctype html>
<html lang="en" style="{vars}">
<head>
<meta charset="utf-8">
<title>New Tab</title>
<style>{BASE_CSS}
body{{height:100vh;overflow:hidden}}
main{{position:relative;z-index:1;height:100%;display:flex;flex-direction:column;align-items:center;justify-content:center;padding:0 20px 8vh}}
.on-image{{color:#fff}}
.clock{{font-family:"Segoe UI Variable Display","Segoe UI",sans-serif;font-weight:300;font-size:clamp(56px,9vw,104px);letter-spacing:-3px;line-height:1;
  font-variant-numeric:tabular-nums;animation:rise .6s cubic-bezier(.2,.8,.2,1) both}}
.date{{margin:10px 0 34px;font-size:16px;opacity:.75;animation:rise .6s .05s cubic-bezier(.2,.8,.2,1) both}}
.brand{{display:flex;align-items:center;gap:14px;font-size:44px;font-weight:650;letter-spacing:-1.5px;margin:0 0 30px;color:inherit;
  animation:rise .6s cubic-bezier(.2,.8,.2,1) both}}
.brand img{{width:58px;height:58px;border-radius:15px;box-shadow:0 5px 18px #0003}}
#form{{width:min(640px,92vw);position:relative;animation:rise .6s .1s cubic-bezier(.2,.8,.2,1) both}}
.search{{display:flex;align-items:center;gap:10px;padding:6px 6px 6px 20px;border-radius:999px;
  background:color-mix(in srgb,var(--elevated) 78%,transparent);border:1px solid color-mix(in srgb,var(--fg) 12%,transparent);
  backdrop-filter:blur(20px) saturate(1.4);box-shadow:0 12px 40px #0000002a;transition:border-color .2s,box-shadow .2s,transform .2s}}
.search:focus-within{{border-color:var(--accent);box-shadow:0 12px 40px #0000002a}}
.search svg{{flex:none;opacity:.6}}
.search input{{flex:1;min-width:0;border:0;outline:0;background:transparent;font-size:17px;padding:10px 0;color:var(--fg)}}
.search input::placeholder{{color:var(--muted)}}
.go{{flex:none;width:44px;height:44px;border-radius:50%;border:0;cursor:pointer;display:grid;place-items:center;
  background:var(--accent);color:var(--on-accent);transition:transform .15s,filter .15s}}
.go:hover{{transform:scale(1.06);filter:brightness(1.08)}} .go:active{{transform:scale(.96)}}
.hint{{position:absolute;right:66px;top:50%;transform:translateY(-50%);font-size:12px;opacity:.45;border:1px solid currentColor;border-radius:5px;padding:1px 6px}}
.search input:not(:placeholder-shown) ~ .hint{{display:none}}
.tiles{{display:grid;grid-template-columns:repeat(auto-fit,96px);justify-content:center;gap:12px;margin-top:40px;width:min(680px,94vw)}}
.tile{{display:flex;flex-direction:column;align-items:center;gap:9px;padding:14px 6px 12px;border-radius:16px;text-decoration:none;color:inherit;
  transition:background .2s,transform .2s;animation:rise .5s cubic-bezier(.2,.8,.2,1) both}}
.tile:hover{{background:color-mix(in srgb,var(--elevated) 70%,transparent);transform:translateY(-3px)}}
.ico{{width:52px;height:52px;border-radius:15px;display:grid;place-items:center;font-weight:650;font-size:21px;
  background:color-mix(in srgb,var(--elevated) 88%,transparent);border:1px solid color-mix(in srgb,var(--fg) 10%,transparent);box-shadow:0 4px 14px #0002;backdrop-filter:blur(12px)}}
.ico img{{width:30px;height:30px;border-radius:6px}}
.name{{font-size:12.5px;max-width:100%;overflow:hidden;text-overflow:ellipsis;white-space:nowrap;opacity:.85}}
.corner{{position:fixed;z-index:2;right:18px;bottom:16px;display:flex;gap:8px}}
.chip{{display:flex;align-items:center;gap:7px;padding:8px 13px;border-radius:999px;font-size:13px;text-decoration:none;color:inherit;
  background:color-mix(in srgb,var(--elevated) 75%,transparent);border:1px solid color-mix(in srgb,var(--fg) 10%,transparent);backdrop-filter:blur(14px);transition:transform .15s}}
.chip:hover{{transform:translateY(-1px)}}
.private{{display:flex;align-items:center;gap:10px;margin-bottom:26px;padding:10px 16px;border-radius:14px;font-size:14px;
  background:color-mix(in srgb,var(--elevated) 82%,transparent);border:1px solid var(--border);animation:rise .5s both}}
.private svg,.chip svg{{width:17px;height:17px;flex:none}}
.context{{position:fixed;z-index:20;min-width:168px;padding:6px;border-radius:12px;background:var(--elevated);color:var(--fg);border:1px solid var(--border);box-shadow:0 14px 36px #0006}}
.context button{{display:flex;align-items:center;gap:9px;width:100%;padding:9px 10px;border:0;border-radius:8px;background:transparent;text-align:left;cursor:pointer;font-size:13px}}
.context button:hover,.context button:focus-visible{{background:color-mix(in srgb,var(--fg) 8%,transparent);outline:none}}
.context button.danger{{color:#ef4444}}
.context svg{{width:16px;height:16px;flex:none}}
dialog{{width:min(420px,calc(100vw - 32px));padding:24px;border:1px solid var(--border);border-radius:18px;background:var(--surface);color:var(--fg);box-shadow:0 24px 70px #0007}}
dialog::backdrop{{background:#0008;backdrop-filter:blur(4px)}}
dialog h2{{margin:0 0 6px;font-size:20px;font-weight:600}}
.dialog-sub{{margin:0 0 20px;color:var(--muted);font-size:13px}}
.field{{display:block;margin-top:14px;font-size:13px;font-weight:550}}
.field input{{display:block;width:100%;margin-top:7px;padding:10px 12px;border:1px solid var(--border);border-radius:10px;background:var(--bg);outline:none;font-size:14px;font-weight:400}}
.field input:focus{{border-color:var(--accent)}}
.dialog-actions{{display:flex;justify-content:flex-end;gap:9px;margin-top:22px}}
.dialog-btn{{padding:8px 15px;border:1px solid var(--border);border-radius:9px;background:var(--bg);cursor:pointer;font-size:13px}}
.dialog-btn:hover{{background:var(--elevated)}}
.dialog-btn.primary{{background:var(--accent);color:var(--on-accent);border-color:transparent}}
</style>
</head>
<body>
<div class="bg" id="bg"><i></i><i></i><i></i></div>
<main id="main">
  <div id="priv" class="private" hidden><svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.8" stroke-linecap="round" stroke-linejoin="round" aria-hidden="true"><rect x="5" y="10" width="14" height="11" rx="2"/><path d="M8 10V7a4 4 0 0 1 8 0v3"/></svg><span>Private window. Aurora won't keep history, cookies or site data after you close it.</span></div>
  <div id="clock" class="clock" hidden></div>
  <div id="date" class="date" hidden></div>
  <h1 id="brand" class="brand" hidden><img src="{logo}" alt="" decoding="async">Aurora</h1>
  <form id="form" autocomplete="off">
    <div class="search">
      <svg width="20" height="20" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round"><circle cx="11" cy="11" r="7"/><path d="m20 20-3.5-3.5"/></svg>
      <input id="q" name="q" placeholder="Search" autofocus spellcheck="false">
      <span class="hint">/</span>
      <button class="go" type="submit" title="Search" aria-label="Search">
        <svg width="20" height="20" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2.4" stroke-linecap="round" stroke-linejoin="round"><path d="M5 12h14M13 6l6 6-6 6"/></svg>
      </button>
    </div>
  </form>
  <div id="tiles" class="tiles"></div>
</main>
<div class="corner">
  <a class="chip" href="http://aurora.settings/#appearance"><svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.8" stroke-linecap="round" stroke-linejoin="round" aria-hidden="true"><path d="M12 3a9 9 0 1 0 0 18h1a2 2 0 0 0 0-4h-.5a1.5 1.5 0 0 1 0-3H16a5 5 0 0 0 5-5c0-3.3-4-6-9-6Z"/><circle cx="7.5" cy="10" r=".7" fill="currentColor"/><circle cx="10" cy="6.8" r=".7" fill="currentColor"/><circle cx="14.5" cy="6.8" r=".7" fill="currentColor"/></svg>Customize</a>
</div>
<div class="context" id="bookmark-menu" role="menu" hidden>
  <button id="edit-bookmark" type="button" role="menuitem"><svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.8" stroke-linecap="round" stroke-linejoin="round" aria-hidden="true"><path d="m14 5 5 5M4 20l3.5-.7L19 7.8a2 2 0 0 0-2.8-2.8L4.7 16.5 4 20Z"/></svg>Edit bookmark</button>
  <button id="remove-bookmark" class="danger" type="button" role="menuitem"><svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.8" stroke-linecap="round" stroke-linejoin="round" aria-hidden="true"><path d="M4 7h16M9 7V4h6v3M7 7l1 14h8l1-14M10 11v6M14 11v6"/></svg>Remove bookmark</button>
</div>
<dialog id="bookmark-dialog" aria-labelledby="bookmark-dialog-title">
  <form id="bookmark-form">
    <h2 id="bookmark-dialog-title">Edit bookmark</h2>
    <p class="dialog-sub">Change the name or address of this shortcut.</p>
    <label class="field">Name<input id="bookmark-title" type="text" maxlength="200" autocomplete="off"></label>
    <label class="field">Address<input id="bookmark-url" type="text" required autocomplete="off" spellcheck="false"></label>
    <div class="dialog-actions"><button class="dialog-btn" id="cancel-edit" type="button">Cancel</button><button class="dialog-btn primary" type="submit">Save</button></div>
  </form>
</dialog>
<div class="toast" id="toast"></div>
<script>
const D = {data};
const $ = (id) => document.getElementById(id);
const BOOKMARKS = D.bookmarks;
// The browser rewrites them in this page after an edit made here; the tiles
// then follow the browser instead of the copy this page was served with.
const shown = () => BOOKMARKS;
window.__auroraBookmarks = (list) => {{ BOOKMARKS.splice(0, BOOKMARKS.length, ...(list || [])); renderTiles(); }};
const send = (m) => window.ipc.postMessage(JSON.stringify(Object.assign({{ kind: "aurora" }}, m)));
let toastTimer;
window.__auroraToast = (msg) => {{
  const t = $("toast"); t.textContent = msg; t.classList.add("show");
  clearTimeout(toastTimer); toastTimer = setTimeout(() => t.classList.remove("show"), 3200);
}};
// The settings page mirrors every change in localStorage; apply the ones that
// only touch this page live instead of waiting for the next reload.
addEventListener("storage", (e) => {{
  if (!D.mirror || e.key !== "aurora.settings" || !e.newValue) return;
  let next;
  try {{ next = JSON.parse(e.newValue).settings || {{}}; }} catch (_) {{ return; }}
  document.documentElement.classList.toggle("still", !next.smooth_animations);
  if ("show_clock" in next && !!next.show_clock !== !$("clock").hidden) {{
    $("clock").hidden = $("date").hidden = !next.show_clock;
    $("brand").hidden = !!next.show_clock;
  }}
}});
if (!D.animate) document.documentElement.classList.add("still");
const bg = $("bg");
bg.classList.add("bg-" + D.background);
const imageBg = ["sunset","ocean","forest","nebula","custom"].includes(D.background);
if (D.background === "custom") bg.style.backgroundImage = `url("${{D.customBackground.replace(/"/g, "%22")}}")`;
if (imageBg) document.body.classList.add("on-image");
if (D.private) $("priv").hidden = false;

if (D.clock) {{
  $("clock").hidden = $("date").hidden = false;
  const tick = () => {{
    const now = new Date();
    $("clock").textContent = now.toLocaleTimeString([], {{ hour: "2-digit", minute: "2-digit" }});
    const h = now.getHours();
    const hello = h < 5 ? "Good night" : h < 12 ? "Good morning" : h < 18 ? "Good afternoon" : "Good evening";
    $("date").textContent = hello + " · " + now.toLocaleDateString([], {{ weekday: "long", month: "long", day: "numeric" }});
  }};
  tick();
  // Hidden tabs (background windows, minimized) skip the timer and the
  // drifting background; a page nobody can see should cost nothing.
  let timer = setInterval(tick, 1000);
  document.addEventListener("visibilitychange", () => {{
    const hidden = document.visibilityState === "hidden";
    document.documentElement.classList.toggle("still", hidden || !D.animate);
    clearInterval(timer);
    if (!hidden) {{ tick(); timer = setInterval(tick, 1000); }}
  }});
  clearInterval(timer);
  if (document.visibilityState !== "hidden") timer = setInterval(tick, 1000);
}} else $("brand").hidden = false;

const q = $("q");
q.placeholder = "Search " + D.engine.name + " or type a URL";
addEventListener("keydown", (e) => {{
  const active = document.activeElement;
  if (e.key !== "/" || active === q || dialog.open || (active && active.matches("input,textarea,select,[contenteditable]"))) return;
  e.preventDefault(); q.focus();
}});
$("form").addEventListener("submit", (e) => {{
  e.preventDefault();
  const v = q.value.trim();
  if (!v) return q.focus();
  if (/^[a-z][\w+.-]*:\/\//i.test(v)) location.href = v;
  else if (!/\s/.test(v) && /^(localhost|[\w-]+(\.[\w-]+)*\.[a-z]{{2,}})(:\d+)?(\/.*)?$/i.test(v))
    location.href = (/^localhost/i.test(v) ? "http://" : "https://") + v;
  else location.href = D.engine.url.replace("{{}}", encodeURIComponent(v));
}});

const tiles = $("tiles");
const menu = $("bookmark-menu");
const dialog = $("bookmark-dialog");
let menuBookmark = null;
let editingBookmark = null;

const hideMenu = () => {{ menu.hidden = true; menuBookmark = null; }};
const showMenu = (b, x, y) => {{
  menuBookmark = b;
  menu.hidden = false;
  const r = menu.getBoundingClientRect();
  menu.style.left = Math.max(8, Math.min(x, innerWidth - r.width - 8)) + "px";
  menu.style.top = Math.max(8, Math.min(y, innerHeight - r.height - 8)) + "px";
  $("edit-bookmark").focus();
}};

const renderTiles = () => {{
  tiles.replaceChildren();
  shown().forEach((b, i) => {{
    const a = document.createElement("a");
    a.className = "tile"; a.href = b.url; a.title = b.title + "\n" + b.url + "\nRight-click to edit or remove";
    a.style.animationDelay = (0.15 + i * 0.03) + "s";
    a.addEventListener("contextmenu", (e) => {{ e.preventDefault(); showMenu(b, e.clientX, e.clientY); }});
    const ico = document.createElement("span"); ico.className = "ico";
    const letter = () => {{ ico.textContent = (b.title.match(/[\p{{L}}\p{{N}}]/u) || ["•"])[0].toUpperCase(); }};
    let host = ""; try {{ host = new URL(b.url).hostname; }} catch (_) {{}}
    if (D.icons && host) {{
      const img = document.createElement("img");
      img.alt = ""; img.decoding = "async"; img.loading = "lazy";
      // Start with the icon in the browser's own (disk) cache; only ask
      // the network again if that fails.
      const direct = "https://www.google.com/s2/favicons?sz=64&domain=" + encodeURIComponent(host);
      img.onerror = () => {{ if (img.src !== direct) {{ img.src = direct; }} else letter(); }};
      img.src = "/favicon?h=" + encodeURIComponent(host);
      ico.appendChild(img);
    }} else letter();
    const name = document.createElement("span"); name.className = "name"; name.textContent = b.title;
    a.append(ico, name); tiles.appendChild(a);
  }});
}};
renderTiles();

$("edit-bookmark").addEventListener("click", () => {{
  if (!menuBookmark) return;
  editingBookmark = menuBookmark;
  hideMenu();
  $("bookmark-title").value = editingBookmark.title;
  $("bookmark-url").value = editingBookmark.url;
  dialog.showModal();
  $("bookmark-title").focus();
  $("bookmark-title").select();
}});
$("remove-bookmark").addEventListener("click", () => {{
  if (!menuBookmark) return;
  const b = menuBookmark;
  hideMenu();
  send({{ cmd: "remove_bookmark", url: b.url }});
  // Replaced in place: the tiles share `BOOKMARKS`, so the browser updating
  // the page (see `mirror_bookmarks`) needs the same array.
  BOOKMARKS.splice(0, BOOKMARKS.length, ...BOOKMARKS.filter((item) => item !== b));
  renderTiles();
  __auroraToast("Bookmark removed");
}});
$("cancel-edit").addEventListener("click", () => dialog.close());
dialog.addEventListener("close", () => {{ editingBookmark = null; }});
$("bookmark-form").addEventListener("submit", (e) => {{
  e.preventDefault();
  if (!editingBookmark) return;
  const title = $("bookmark-title").value.trim();
  const input = $("bookmark-url").value.trim();
  let parsed;
  try {{ parsed = new URL(/^[a-z][a-z\d+.-]*:/i.test(input) ? input : "https://" + input); }}
  catch (_) {{ return __auroraToast("Enter a valid web address."); }}
  if (!["http:", "https:"].includes(parsed.protocol) || !parsed.hostname)
    return __auroraToast("Bookmarks must be http or https addresses.");
  const b = editingBookmark;
  const newUrl = parsed.href;
  send({{ cmd: "edit_bookmark", url: b.url, title, new_url: newUrl }});
  b.title = title || newUrl;
  b.url = newUrl;
  dialog.close();
  renderTiles();
  __auroraToast("Bookmark updated");
}});

document.addEventListener("pointerdown", (e) => {{ if (!menu.hidden && !menu.contains(e.target)) hideMenu(); }});
document.addEventListener("keydown", (e) => {{ if (e.key === "Escape") hideMenu(); }});
menu.addEventListener("contextmenu", (e) => e.preventDefault());
addEventListener("resize", hideMenu);
addEventListener("blur", hideMenu);
</script>
</body>
</html>"##,
        vars = themes::css_vars(lib.palette),
        data = script_json(&data),
        logo = crate::browser::LOGO_PATH,
    )
}

pub fn settings(lib: &mut Library) -> String {
    // The update result is shown once, on the page that asked for it.
    let update = lib.update_message.take();
    let engines: Vec<_> = SEARCH_ENGINES.iter().map(|e| json!({ "id": e.id, "name": e.name })).collect();
    let backgrounds: Vec<_> = BACKGROUNDS.iter().map(|(id, name)| json!({ "id": id, "name": name })).collect();
    let data = json!({
        "settings": lib.settings,
        "palettes": serde_json::from_str::<serde_json::Value>(&themes::palettes_json()).unwrap_or_default(),
        "engines": engines,
        "backgrounds": backgrounds,
        "counts": { "bookmarks": lib.bookmarks.len(), "history": lib.history.len() },
        "version": env!("CARGO_PKG_VERSION"),
        "engineVersion": wry::webview_version().unwrap_or_else(|_| "unknown".into()),
        "dataDir": crate::storage::data_dir().display().to_string(),
        "releaseRepo": crate::update::REPO,
        "update": update,
    });

    format!(
        r##"<!doctype html>
<html lang="en" style="{vars}">
<head>
<meta charset="utf-8">
<title>Settings</title>
<style>{BASE_CSS}
body{{display:flex;min-height:100vh}}
nav{{position:sticky;top:0;height:100vh;width:240px;flex:none;padding:28px 14px;border-right:1px solid var(--border);background:var(--surface)}}
nav h1{{display:flex;align-items:center;gap:10px;font-size:20px;margin:0 10px 24px;font-weight:650;letter-spacing:-.3px;color:var(--fg)}}
nav h1 img{{width:30px;height:30px;border-radius:8px}}
nav a{{display:flex;align-items:center;gap:10px;padding:9px 12px;margin:2px 0;border-radius:10px;color:var(--fg);text-decoration:none;font-size:14px;opacity:.8;transition:background .15s,opacity .15s}}
nav a:hover{{background:var(--elevated);opacity:1}}
nav a.on{{background:color-mix(in srgb,var(--accent) 16%,transparent);opacity:1;color:var(--accent)}}
nav a svg{{width:17px;height:17px;flex:none}}
.content{{flex:1;max-width:900px;padding:34px 44px 80px}}
section{{margin-bottom:38px;scroll-margin-top:24px;animation:rise .45s cubic-bezier(.2,.8,.2,1) both}}
h2{{font-size:22px;font-weight:600;margin:0 0 4px;letter-spacing:-.3px}}
.sub{{color:var(--muted);font-size:14px;margin:0 0 18px}}
.card{{background:var(--elevated);border:1px solid var(--border);border-radius:16px;overflow:hidden}}
.row{{display:flex;align-items:center;gap:16px;padding:15px 18px;border-top:1px solid var(--border)}}
.row:first-child{{border-top:0}}
.row .t{{flex:1}} .row .t b{{display:block;font-weight:550;font-size:14.5px}} .row .t small{{color:var(--muted);font-size:13px}}
.grid{{display:grid;grid-template-columns:repeat(auto-fill,minmax(150px,1fr));gap:12px}}
.pick{{cursor:pointer;border-radius:14px;border:2px solid transparent;background:var(--elevated);padding:8px;transition:transform .15s,border-color .15s;text-align:left}}
.pick:hover{{transform:translateY(-2px)}} .pick.on{{border-color:var(--accent)}}
.pick .pv{{height:78px;border-radius:9px;position:relative;overflow:hidden;display:flex;align-items:flex-end;gap:5px;padding:9px;border:1px solid #0001}}
.pick .pv span{{width:18px;height:18px;border-radius:50%;box-shadow:0 1px 3px #0003}}
.pick .bg{{position:absolute;inset:0;z-index:0;border-radius:9px}}
.pick .label{{display:block;padding:8px 4px 2px;font-size:13px;font-weight:500}}
.switch{{position:relative;width:44px;height:26px;flex:none}}
.switch input{{opacity:0;width:0;height:0}}
.switch i{{position:absolute;inset:0;border-radius:99px;background:color-mix(in srgb,var(--fg) 22%,transparent);transition:.2s;cursor:pointer}}
.switch i::after{{content:"";position:absolute;left:3px;top:3px;width:20px;height:20px;border-radius:50%;background:#fff;box-shadow:0 1px 3px #0004;transition:.2s cubic-bezier(.3,1.4,.5,1)}}
.switch input:checked+i{{background:var(--accent)}} .switch input:checked+i::after{{transform:translateX(18px)}}
.btn{{border:1px solid var(--border);background:var(--bg);padding:8px 14px;border-radius:10px;cursor:pointer;font-size:13.5px;transition:background .15s,transform .1s;white-space:nowrap}}
.btn:hover{{background:var(--surface)}} .btn:active{{transform:scale(.97)}}
.btn.primary{{background:var(--accent);color:var(--on-accent);border-color:transparent}}
.btn.danger{{color:#ef4444}}
.engines{{display:flex;flex-wrap:wrap;gap:8px;padding:16px 18px}}
.engines label{{padding:8px 14px;border-radius:999px;border:1px solid var(--border);cursor:pointer;font-size:14px;transition:.15s}}
.engines input{{display:none}} .engines input:checked+span{{color:var(--accent);font-weight:600}}
.engines label:has(input:checked){{border-color:var(--accent);background:color-mix(in srgb,var(--accent) 12%,transparent)}}
input[type=url]{{flex:1;min-width:0;padding:9px 12px;border-radius:10px;border:1px solid var(--border);background:var(--bg);outline:0}}
input[type=url]:focus{{border-color:var(--accent)}}
kbd{{font-family:"Cascadia Code",Consolas,monospace;font-size:12px;padding:2px 7px;border-radius:6px;border:1px solid var(--border);background:var(--bg)}}
.keys{{display:grid;grid-template-columns:1fr auto;gap:10px 20px;padding:16px 18px;font-size:14px}}
code{{font-family:"Cascadia Code",Consolas,monospace;font-size:12.5px;color:var(--muted)}}
</style>
</head>
<body>
<nav>
  <h1><img src="{logo}" alt="" decoding="async"><span>Aurora settings</span></h1>
  <a href="#appearance"><svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.8" stroke-linecap="round" stroke-linejoin="round" aria-hidden="true"><path d="M12 3a9 9 0 1 0 0 18h1a2 2 0 0 0 0-4h-.5a1.5 1.5 0 0 1 0-3H16a5 5 0 0 0 5-5c0-3.3-4-6-9-6Z"/><circle cx="7.5" cy="10" r=".7" fill="currentColor"/><circle cx="10" cy="6.8" r=".7" fill="currentColor"/><circle cx="14.5" cy="6.8" r=".7" fill="currentColor"/></svg>Appearance</a>
  <a href="#search"><svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.8" stroke-linecap="round" aria-hidden="true"><circle cx="11" cy="11" r="7"/><path d="m20 20-3.5-3.5"/></svg>Search</a>
  <a href="#browsing"><svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.8" stroke-linecap="round" stroke-linejoin="round" aria-hidden="true"><circle cx="12" cy="12" r="9"/><path d="m15.5 8.5-2 5-5 2 2-5 5-2Z"/></svg>Browsing</a>
  <a href="#privacy"><svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.8" stroke-linecap="round" stroke-linejoin="round" aria-hidden="true"><path d="M12 3 5 6v5c0 4.5 2.8 8 7 10 4.2-2 7-5.5 7-10V6l-7-3Z"/><path d="m9 12 2 2 4-4"/></svg>Privacy</a>
  <a href="#import"><svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.8" stroke-linecap="round" stroke-linejoin="round" aria-hidden="true"><path d="M12 3v12m-4-4 4 4 4-4M5 15v5h14v-5"/></svg>Import</a>
  <a href="#shortcuts"><svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.8" stroke-linecap="round" stroke-linejoin="round" aria-hidden="true"><rect x="3" y="5" width="18" height="14" rx="2"/><path d="M7 9h.01M11 9h.01M15 9h.01M7 13h.01M11 13h6"/></svg>Shortcuts</a>
  <a href="#about"><svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.8" stroke-linecap="round" aria-hidden="true"><circle cx="12" cy="12" r="9"/><path d="M12 11v5M12 8h.01"/></svg>About</a>
</nav>
<div class="content">
  <section id="appearance">
    <h2>Appearance</h2><p class="sub">Themes change the whole browser, not just this page.</p>
    <div class="grid" id="themes"></div>
    <h2 style="margin-top:28px;font-size:17px">New tab background</h2><p class="sub">Shown behind the clock and your bookmarks.</p>
    <div class="grid" id="backgrounds"></div>
    <div class="card" style="margin-top:14px">
      <div class="row"><div class="t"><b>Your own image</b><small>Paste an image URL, or upload a file from your PC.</small></div></div>
      <div class="row">
        <input type="url" id="bgurl" placeholder="https://example.com/wallpaper.jpg">
        <button class="btn" id="bgurlset">Use URL</button>
        <label class="btn primary">Upload…<input type="file" id="bgfile" accept="image/*" hidden></label>
      </div>
    </div>
    <div class="card" style="margin-top:14px" id="appearance-toggles"></div>
  </section>

  <section id="search">
    <h2>Search engine</h2><p class="sub">Used by the address bar and the new tab page.</p>
    <div class="card"><div class="engines" id="engines"></div></div>
  </section>

  <section id="browsing">
    <h2>Browsing</h2><p class="sub">Start-up, toolbar and how pages look.</p>
    <div class="card" id="browsing-toggles"></div>
  </section>

  <section id="privacy">
    <h2>Privacy and security</h2><p class="sub">Your data stays on this PC, in <code id="datadir"></code>.</p>
    <div class="card" id="privacy-toggles"></div>
    <div class="card" style="margin-top:14px">
      <div class="row"><div class="t"><b>Browsing history</b><small id="histcount"></small></div><button class="btn danger" data-cmd="clear_history" data-confirm="Delete all browsing history?">Clear history</button></div>
      <div class="row"><div class="t"><b>Cookies, cache and site data</b><small>Signs you out of every site.</small></div><button class="btn danger" data-cmd="clear_data" data-confirm="Delete all cookies, cache and site data? You will be signed out everywhere.">Clear browsing data</button></div>
    </div>
  </section>

  <section id="import">
    <h2>Import</h2><p class="sub">Bring your bookmarks over from another browser.</p>
    <div class="card">
      <div class="row"><div class="t"><b>Google Chrome</b><small>Bookmarks from every Chrome profile, including ones synced to your Google account. Passwords and cookies are encrypted by Chrome and cannot be imported.</small></div><button class="btn primary" data-cmd="import_chrome">Import</button></div>
    </div>
  </section>

  <section id="shortcuts">
    <h2>Keyboard shortcuts</h2><p class="sub">These work everywhere, including inside pages.</p>
    <div class="card"><div class="keys" id="keys"></div></div>
  </section>

  <section id="about">
    <h2>About</h2><p class="sub">A fast browser with a native Rust interface.</p>
    <div class="card">
      <div class="row"><div class="t"><b>Aurora <span id="ver"></span></b><small>Interface built with GPUI · pages rendered by Microsoft Edge WebView2 <span id="ever"></span></small></div>
        <button class="btn" data-cmd="open_devtools">Open DevTools</button>
        <button class="btn" id="update">Check for updates</button></div>
      <div class="row"><div class="t"><b>Updates</b><small>From <span id="ghrepo"></span>. Aurora replaces only its own files, keeps your data, and restarts when done.</small></div>
        <button class="btn" data-cmd="open_downloads">Show downloads</button></div>
    </div>
  </section>
</div>
<div class="toast" id="toast"></div>
<script>
const D = {data};
const $ = (id) => document.getElementById(id);
const S = D.settings;

if (!S.smooth_animations) document.documentElement.classList.add("still");
const send = (m) => window.ipc.postMessage(JSON.stringify(Object.assign({{ kind: "aurora" }}, m)));
// Aurora keeps the settings in its own process; this page only renders them.
// Every command below is followed by `reload`, which reopens this page so the
// new values arrive (see `serve` in tab.rs) with the position kept. Changes
// are also mirrored in localStorage, which the open new tab page listens to.
const MIRROR = "aurora.settings";
const notify = () => {{ try {{ localStorage.setItem(MIRROR, JSON.stringify({{ settings: S, at: Date.now() }})); }} catch (_) {{}} }};
const CARRY = ["s", "pick"];
const reload = (extra) => {{
  notify();
  const paras = new URLSearchParams(location.search);
  for (const k of CARRY) paras.delete(k);
  if (extra && extra.keepPick !== false) paras.set("pick", extra.pick || "");
  paras.set("s", String(Math.round(window.scrollY || 0)));
  location.replace(location.pathname + "?" + paras.toString());
}};
const set = (key, value, extra) => {{ S[key] = value; send({{ cmd: "set", key, value }}); reload(extra); }};
let toastTimer;
window.__auroraToast = (msg) => {{
  const t = $("toast"); t.textContent = msg; t.classList.add("show");
  clearTimeout(toastTimer); toastTimer = setTimeout(() => t.classList.remove("show"), 3200);
}};
const el = (tag, cls, text) => {{ const e = document.createElement(tag); if (cls) e.className = cls; if (text != null) e.textContent = text; return e; }};

// Themes
const themes = $("themes");
const systemVars = () => D.palettes.find(p => p.id === (matchMedia("(prefers-color-scheme: dark)").matches ? "aurora" : "light")).vars;
const applyVars = (id) => {{ document.documentElement.setAttribute("style", id === "system" ? systemVars() : D.palettes.find(p => p.id === id).vars); }};
const themeCards = [{{ id: "system", name: "Match Windows", swatch: ["#10131f", "#ffffff", "#8b7bff", "#5b5bf0"], split: true }}, ...D.palettes];
themeCards.forEach(p => {{
  const b = el("button", "pick" + (S.theme === p.id ? " on" : ""));
  const pv = el("div", "pv");
  pv.style.background = p.split ? "linear-gradient(135deg,#10131f 50%,#f1f3f7 50%)" : p.swatch[0];
  p.swatch.slice(1).forEach(c => {{ const s = el("span"); s.style.background = c; pv.appendChild(s); }});
  b.append(pv, el("span", "label", p.name));
  b.onclick = () => {{ themes.querySelectorAll(".pick").forEach(x => x.classList.remove("on")); b.classList.add("on"); set("theme", p.id); applyVars(p.id); }};
  themes.appendChild(b);
}});

// Backgrounds
const bgs = $("backgrounds");
D.backgrounds.forEach(bgd => {{
  const b = el("button", "pick" + (S.background === bgd.id ? " on" : ""));
  const pv = el("div", "pv");
  const layer = el("div", "bg bg-" + bgd.id); layer.innerHTML = "<i></i><i></i><i></i>";
  if (bgd.id === "custom") {{ layer.style.backgroundImage = `url("${{(S.background_url || "http://aurora.newtab/background").replace(/"/g, "%22")}}")`; layer.style.background = layer.style.background || ""; }}
  pv.appendChild(layer);
  b.append(pv, el("span", "label", bgd.name));
  b.onclick = () => {{ bgs.querySelectorAll(".pick").forEach(x => x.classList.remove("on")); b.classList.add("on"); set("background", bgd.id, {{ pick: bgd.name }}); }};
  bgs.appendChild(b);
}});
const pickCustom = () => {{ bgs.querySelectorAll(".pick").forEach((x, i) => x.classList.toggle("on", D.backgrounds[i].id === "custom")); }};
$("bgurl").value = S.background_url || "";
$("bgurlset").onclick = () => {{
  const v = $("bgurl").value.trim();
  if (v && !/^https?:\/\//i.test(v)) return __auroraToast("The URL must start with https://");
  set("background_url", v); set("background", "custom"); pickCustom(); __auroraToast(v ? "Background set" : "Using your uploaded image");
}};
$("bgfile").onchange = (e) => {{
  const f = e.target.files[0]; if (!f) return;
  if (f.size > 15 * 1024 * 1024) return __auroraToast("Please pick an image under 15 MB");
  const r = new FileReader();
  r.onload = () => {{ send({{ cmd: "background_image", data: String(r.result).split(",")[1] || "" }}); S.background_url = ""; $("bgurl").value = ""; S.background = "custom"; pickCustom(); }};
  r.readAsDataURL(f);
}};

// Toggles
const toggle = (parent, key, title, desc) => {{
  const row = el("div", "row"); const t = el("div", "t"); t.append(el("b", null, title), el("small", null, desc));
  const sw = el("label", "switch"); const input = el("input"); input.type = "checkbox"; input.checked = !!S[key];
  input.onchange = () => {{ set(key, input.checked); if (key === "smooth_animations") document.documentElement.classList.toggle("still", !input.checked); }};
  sw.append(input, el("i")); row.append(t, sw); $(parent).appendChild(row);
}};
toggle("appearance-toggles", "show_clock", "Clock on the new tab page", "Shows the time and a greeting above the search box.");
toggle("appearance-toggles", "smooth_animations", "Animations", "Animated backgrounds, tab transitions and the sliding sidebar.");
toggle("browsing-toggles", "restore_session", "Continue where you left off", "Reopen your tabs and windows when Aurora starts.");
toggle("browsing-toggles", "show_bookmarks_bar", "Show the bookmarks bar", "Also with Ctrl+Shift+B.");
toggle("browsing-toggles", "site_icons", "Site icons", "Loads small site logos from Google's favicon service for tabs and bookmarks.");
toggle("browsing-toggles", "show_dev_tools_button", "Developer tools button", "Shows a DevTools button in the toolbar. F12 always works.");
toggle("privacy-toggles", "save_passwords", "Offer to save passwords", "Uses the Edge password manager built into WebView2, and fills in forms. Applies to new tabs.");

// Search engines
const engines = $("engines");
D.engines.forEach(e => {{
  const l = el("label"); const i = el("input"); i.type = "radio"; i.name = "engine"; i.checked = S.search_engine === e.id;
  i.onchange = () => set("search_engine", e.id);
  l.append(i, el("span", null, e.name)); engines.appendChild(l);
}});

// Commands
document.querySelectorAll("[data-cmd]").forEach(b => b.onclick = () => {{
  if (b.dataset.confirm && !confirm(b.dataset.confirm)) return;
  send({{ cmd: b.dataset.cmd }});
}});

// Shortcuts
[["New tab","Ctrl+T"],["New window","Ctrl+N"],["New private window","Ctrl+Shift+N"],["Close tab","Ctrl+W"],["Reopen closed tab","Ctrl+Shift+T"],
 ["Next / previous tab","Ctrl+Tab / Ctrl+Shift+Tab"],["Go to tab 1–8, last tab","Ctrl+1…8, Ctrl+9"],["Address bar","Ctrl+L"],["Find in page","Ctrl+F"],
 ["Bookmark this page","Ctrl+D"],["Bookmarks","Ctrl+Shift+O"],["Bookmarks bar","Ctrl+Shift+B"],["History","Ctrl+H"],["Downloads","Ctrl+J"],
 ["Zoom in / out / reset","Ctrl + / Ctrl − / Ctrl+0"],["Full screen","F11"],["Developer tools","F12"],["View source","Ctrl+U"],["Settings","Ctrl+,"],["Reload","F5"]]
.forEach(([a, k]) => {{ $("keys").append(el("span", null, a), el("kbd", null, k)); }});

$("ver").textContent = D.version; $("ever").textContent = D.engineVersion; $("datadir").textContent = D.dataDir;
$("ghrepo").textContent = "github.com/" + D.releaseRepo;
$("update").onclick = () => window.ipc.postMessage(JSON.stringify({{ kind: "aurora", cmd: "check_for_updates" }}));
if (D.update) __auroraToast(D.update);
$("histcount").textContent = D.counts.history + " pages visited.";

$("bgurl").value = S.background_url || "";

// Back from a reload: the scroll position and the card just picked.
const paras = new URLSearchParams(location.search);
if (paras.get("s")) requestAnimationFrame(() => window.scrollTo(0, Number(paras.get("s")) || 0));
const picked = paras.get("pick");

if (picked) bgs.querySelectorAll(".pick").forEach(p => p.classList.toggle("on", p.lastChild.textContent === picked));

// Highlight the section in view.
const links = [...document.querySelectorAll("nav a")];
const mark = () => {{
  let cur = links[0];
  for (const a of links) {{ const s = document.querySelector(a.getAttribute("href")); if (s.getBoundingClientRect().top < 140) cur = a; }}
  links.forEach(a => a.classList.toggle("on", a === cur));
}};
addEventListener("scroll", mark); mark();
</script>
</body>
</html>"##,
        vars = themes::css_vars(lib.palette),
        data = script_json(&data),
        logo = crate::browser::LOGO_PATH,
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn json_cannot_close_the_script() {
        let s = script_json(&json!({ "t": "</script><script>alert(1)</script>" }));
        assert!(!s.contains("</script>"));
    }
}
