import re

def edit(path, fn):
    with open(path, encoding='utf-8') as f:
        s = f.read()
    s2 = fn(s)
    with open(path, 'w', encoding='utf-8') as f:
        f.write(s2)

edit('src/page.rs', lambda s: s.replace(
    "use crate::layout::{layout_document, ImageData, LayoutResult};",
    "use crate::layout::{ImageData, LayoutResult};"))

def net(s):
    return s.replace(
        "    if let Some(b) = base {\n        if !s.contains(char::is_whitespace) {\n            if let Ok(u) = Url::parse(base).and_then(|bu| bu.join(s)) {",
        "    if let Some(b) = base {\n        if !s.contains(char::is_whitespace) {\n            if let Ok(u) = Url::parse(b).and_then(|bu| bu.join(s)) {")
edit('src/net.rs', net)

def paint(s):
    s = s.replace("self.draw_image(img, self.clip, &self.clip);", "self.draw_image(img, self.clip, self.clip);")
    s = s.replace("self.draw_image(img, rect, &clip);", "self.draw_image(img, rect, clip);")
    s = s.replace("self.draw_image(img, target, &clip);", "self.draw_image(img, target, clip);")
    s = s.replace("if b.border_width[i] <= 0.0 || b.border_style[i] == BorderStyle::None {",
                  "if b.style.border_width[i] <= 0.0 || b.style.border_style[i] == BorderStyle::None {")
    s = s.replace("let color = scale_alpha(b.border_color[i], opacity);",
                  "let color = scale_alpha(b.style.border_color[i], opacity);")
    s = s.replace("match b.border_style[i] {", "match b.style.border_style[i] {")
    s = s.replace("let dash = if b.border_style[i] == BorderStyle::Dotted { 2.0 } else { 5.0 };",
                  "let dash: f32 = if b.style.border_style[i] == BorderStyle::Dotted { 2.0 } else { 5.0 };")
    s = s.replace("let dest = target.intersect(clip);", "let dest = target.intersect(&clip);")
    return s
edit('src/paint.rs', paint)

def util(s):
    first = s.find('"lightsteelblue" => (176, 196, 222),')
    second = s.find('"lightsteelblue" => (176, 196, 222),', first + 1)
    if second != -1:
        line_end = s.find('\n', second)
        s = s[:second] + s[line_end + 1:]
    return s
edit('src/util.rs', util)

edit('src/css.rs', lambda s: s.replace(
    "                            media,\n                            order: sheet.order,",
    "                            media: media.clone(),\n                            order: sheet.order,"))

def style(s):
    s = s.replace(
        "for (idx, val) in [(t, parts[t]), (r, parts[r]), (b, parts[b]), (l, parts[l])] {\n                    self.insert(names[idx], val.as_str(), prio);\n                }",
        "for (idx, val) in [(t, &parts[t]), (r, &parts[r]), (b, &parts[b]), (l, &parts[l])] {\n                    self.insert(names[idx], val, prio);\n                }")
    s = s.replace(
        "    let get = |name: &str| -> Option<String> {\n        winners.get(name).map(|v| resolve_var(v, &style.custom, 0))\n    };",
        "    let custom_props = style.custom.clone();\n    let get = |name: &str| -> Option<String> {\n        winners.get(name).map(|v| resolve_var(v, &custom_props, 0))\n    };")
    return s
edit('src/style.rs', style)

def layout(s):
    old = '''        if let Some(body) = self
            .doc
            .walk(self.doc.root)
            .into_iter()
            .find(|&n| self.doc.tag(n) == Some("body"))
        {
            if let Some(body_box) = root.find_node_mut(body) {
                if body_box.style.background_color.a > 0 && root.style.background_color.a == 0 {
                    root.style.background_color = body_box.style.background_color;
                }
                if body_box.style.background_image.is_some() && root.bg_image.is_none() {
                    root.bg_image = body_box.bg_image.clone();
                }
            }
        }'''
    new = '''        if let Some(body) = self
            .doc
            .walk(self.doc.root)
            .into_iter()
            .find(|&n| self.doc.tag(n) == Some("body"))
        {
            let mut body_bg_image = None;
            let mut body_bg_color = Color::TRANSPARENT;
            let mut has_bg_image = false;
            if let Some(body_box) = root.find_node_mut(body) {
                body_bg_image = body_box.bg_image.clone();
                body_bg_color = body_box.style.background_color;
                has_bg_image = body_box.style.background_image.is_some();
            }
            if body_bg_color.a > 0 && root.style.background_color.a == 0 {
                root.style.background_color = body_bg_color;
            }
            if has_bg_image && root.bg_image.is_none() {
                root.bg_image = body_bg_image;
            }
        }'''
    s = s.replace(old, new)
    old = '''            let mut widths_used = 0.0f32;
            let mut heights: Vec<f32> = Vec::with_capacity(n);
            for child in &b.children {
                let w = match child.style.width {
                    Length::Px(v) => v,
                    Length::Percent(p) => inner_w * p,
                    Length::Auto => inner_w,
                };
                let mut cb = std::mem::replace(
                    &mut b.children[widths_used as usize],
                    LayoutBox::new(b.node, ComputedStyle::default(), BoxKind::Block),
                );
                cb.style.width = Length::Px(w);
                self.layout_block(&mut cb, cx, cy, w, false);
                heights.push(cb.rect.h);
                b.children[widths_used as usize] = cb;
                widths_used += 1.0;
            }'''
    new = '''            let mut heights: Vec<f32> = Vec::with_capacity(n);
            for i in 0..n {
                let w = match b.children[i].style.width {
                    Length::Px(v) => v,
                    Length::Percent(p) => inner_w * p,
                    Length::Auto => inner_w,
                };
                let mut cb = std::mem::replace(
                    &mut b.children[i],
                    LayoutBox::new(b.node, ComputedStyle::default(), BoxKind::Block),
                );
                cb.style.width = Length::Px(w);
                self.layout_block(&mut cb, cx, cy, w, false);
                heights.push(cb.rect.h);
                b.children[i] = cb;
            }'''
    s = s.replace(old, new)
    s = s.replace("            BoxKind::Text(text) => {\n                let mut w = 0.0;",
                  "            BoxKind::Text(text) => {\n                let mut w: f32 = 0.0;")
    return s
edit('src/layout.rs', layout)

def browser(s):
    s = s.replace('''        if let Some(page) = &mut self.page {
            let vp = self.viewport();
            if (page.viewport.0 - vp.0).abs() > 0.5 || (page.viewport.1 - vp.1).abs() > 0.5 {''',
'''        let vp = self.viewport();
        if let Some(page) = &mut self.page {
            if (page.viewport.0 - vp.0).abs() > 0.5 || (page.viewport.1 - vp.1).abs() > 0.5 {''')
    s = s.replace('''            let (page_opt, fonts) = (&mut self.page, &mut self.fonts);
            if let Some(page) = page_opt {
                page.relayout(fonts, self.viewport());
            }''',
'''            let vp = self.viewport();
            let (page_opt, fonts) = (&mut self.page, &mut self.fonts);
            if let Some(page) = page_opt {
                page.relayout(fonts, vp);
            }''')
    return s
edit('src/browser.rs', browser)
print("patched")
