//! Built-in pages served through the `aurora` custom protocol.

use crate::storage::Bookmark;

fn escape(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    for c in s.chars() {
        match c {
            '&' => out.push_str("&amp;"),
            '<' => out.push_str("&lt;"),
            '>' => out.push_str("&gt;"),
            '"' => out.push_str("&quot;"),
            '\'' => out.push_str("&#39;"),
            _ => out.push(c),
        }
    }
    out
}

fn initial(title: &str) -> String {
    title
        .chars()
        .find(|c| c.is_alphanumeric())
        .map(|c| c.to_uppercase().collect())
        .unwrap_or_else(|| "•".into())
}

pub fn new_tab(bookmarks: &[Bookmark]) -> String {
    let tiles: String = bookmarks
        .iter()
        // Only plain web links: a bookmark must never become a javascript: link here.
        .filter(|b| b.url.starts_with("https://") || b.url.starts_with("http://"))
        .take(12)
        .map(|b| {
            format!(
                r#"<a class="tile" href="{url}"><span class="icon">{initial}</span><span class="name">{title}</span></a>"#,
                url = escape(&b.url),
                initial = escape(&initial(&b.title)),
                title = escape(&b.title),
            )
        })
        .collect();

    format!(
        r#"<!doctype html>
<html lang="en">
<head>
<meta charset="utf-8">
<title>New Tab</title>
<style>
  :root {{ color-scheme: light dark; --bg:#f6f7f9; --fg:#1d2128; --muted:#6b7280; --card:#ffffff; --line:#e3e6ea; --accent:#5b6cff; }}
  @media (prefers-color-scheme: dark) {{ :root {{ --bg:#16181d; --fg:#e8eaed; --muted:#9aa0a6; --card:#22252b; --line:#30343b; --accent:#8b97ff; }} }}
  * {{ box-sizing: border-box; }}
  html, body {{ margin:0; height:100%; background:var(--bg); color:var(--fg); font-family:"Segoe UI Variable", "Segoe UI", system-ui, sans-serif; }}
  main {{ min-height:100%; display:flex; flex-direction:column; align-items:center; padding-top:18vh; }}
  h1 {{ font-weight:600; font-size:44px; letter-spacing:-1px; margin:0 0 28px;
        background:linear-gradient(90deg,#5b6cff,#b05bff 50%,#ff7a59); -webkit-background-clip:text; background-clip:text; color:transparent; }}
  form {{ width:min(620px, 90vw); }}
  input {{ width:100%; font:inherit; font-size:16px; padding:14px 20px; border-radius:999px; border:1px solid var(--line);
           background:var(--card); color:var(--fg); outline:none; box-shadow:0 1px 3px rgba(0,0,0,.06); }}
  input:focus {{ border-color:var(--accent); box-shadow:0 0 0 3px color-mix(in srgb, var(--accent) 25%, transparent); }}
  .tiles {{ display:flex; flex-wrap:wrap; justify-content:center; gap:14px; margin-top:40px; width:min(720px, 92vw); }}
  .tile {{ width:104px; padding:14px 8px; border-radius:14px; text-decoration:none; color:var(--fg); display:flex; flex-direction:column; align-items:center; gap:8px; }}
  .tile:hover {{ background:var(--card); }}
  .icon {{ width:48px; height:48px; border-radius:50%; display:grid; place-items:center; font-weight:600; font-size:20px; background:var(--card); border:1px solid var(--line); }}
  .name {{ font-size:13px; max-width:100%; overflow:hidden; text-overflow:ellipsis; white-space:nowrap; color:var(--muted); }}
</style>
</head>
<body>
<main>
  <h1>Aurora</h1>
  <form action="https://www.google.com/search" method="get">
    <input name="q" placeholder="Search the web" autocomplete="off">
  </form>
  <div class="tiles">{tiles}</div>
</main>
</body>
</html>"#
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn bookmarks_are_escaped_and_filtered() {
        let html = new_tab(&[
            Bookmark { title: "<b>x</b>".into(), url: "https://a.com/?a=1&b=\"2\"".into() },
            Bookmark { title: "evil".into(), url: "javascript:alert(1)".into() },
        ]);
        assert!(html.contains("&lt;b&gt;x&lt;/b&gt;"));
        assert!(html.contains("https://a.com/?a=1&amp;b=&quot;2&quot;"));
        assert!(!html.contains("javascript:"));
    }
}
