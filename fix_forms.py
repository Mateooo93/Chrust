p = 'src/util.rs'
s = open(p, encoding='utf-8').read()
s = s.replace('''pub fn escape_html(s: &str) -> String {''', '''pub fn url_encode_component(s: &str) -> String {
    let mut out = String::new();
    for b in s.as_bytes() {
        match b {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'~' => out.push(*b as char),
            b' ' => out.push('+'),
            _ => out.push_str(&format!("%{:02X}", b)),
        }
    }
    out
}

pub fn escape_html(s: &str) -> String {''')
open(p, 'w', encoding='utf-8').write(s)

p = 'src/page.rs'
s = open(p, encoding='utf-8').read()
s = s.replace('''    pub fn link_href(&self, node: usize) -> Option<String> {''', '''    pub fn ancestor_tag(&self, node: usize, tag: &str) -> Option<usize> {
        let doc = self.dom.borrow();
        let mut cur = Some(node);
        while let Some(n) = cur {
            if doc.tag(n) == Some(tag) {
                return Some(n);
            }
            cur = doc.nodes[n].parent;
        }
        None
    }

    pub fn take_pending_navigation(&mut self) -> Option<String> {
        self.js.as_mut().and_then(|js| js.take_pending_navigation())
    }

    pub fn has_js_timers(&self) -> bool {
        self.js
            .as_ref()
            .map(|js| !js.shared.timers.borrow().is_empty())
            .unwrap_or(false)
    }

    pub fn js_tick(&mut self, fonts: &mut FontStore) -> bool {
        if let Some(js) = &mut self.js {
            js.tick_timers();
        }
        let changed = self.take_js_changed();
        if changed {
            let vp = self.viewport;
            self.relayout(fonts, vp);
        }
        changed
    }

    pub fn form_submit_url(&self, form: usize) -> Option<String> {
        let doc = self.dom.borrow();
        let action = doc.attr(form, "action").unwrap_or("").to_string();
        let action_url = if action.is_empty() {
            self.data.url.clone()
        } else {
            resolve_url(&self.data.base_url, action.trim())
        };
        let method = doc
            .attr(form, "method")
            .unwrap_or("get")
            .to_ascii_lowercase();
        let mut pairs: Vec<(String, String)> = Vec::new();
        let mut push = |name: String, value: String| {
            if !name.is_empty() {
                pairs.push((name, value));
            }
        };
        for n in doc.walk(form) {
            match doc.tag(n) {
                Some("input") => {
                    if doc.attr(n, "disabled").is_some() {
                        continue;
                    }
                    let name = doc.attr(n, "name").unwrap_or("").to_string();
                    if name.is_empty() {
                        continue;
                    }
                    let ty = doc.attr(n, "type").unwrap_or("text").to_ascii_lowercase();
                    if ty == "submit" || ty == "button" || ty == "reset" || ty == "file" || ty == "image" {
                        continue;
                    }
                    if (ty == "checkbox" || ty == "radio") && doc.attr(n, "checked").is_none() {
                        continue;
                    }
                    let value = doc.attr(n, "value").unwrap_or("").to_string();
                    push(name, value);
                }
                Some("textarea") => {
                    let name = doc.attr(n, "name").unwrap_or("").to_string();
                    let value = doc.text_content(n);
                    push(name, value);
                }
                Some("select") => {
                    let name = doc.attr(n, "name").unwrap_or("").to_string();
                    if name.is_empty() {
                        continue;
                    }
                    let mut value = String::new();
                    for c in &doc.nodes[n].children {
                        if doc.tag(*c) == Some("option") {
                            if doc.attr(*c, "selected").is_some() || value.is_empty() {
                                let t = doc.text_content(*c).trim().to_string();
                                value = doc.attr(*c, "value").unwrap_or(&t).to_string();
                                if doc.attr(*c, "selected").is_some() {
                                    break;
                                }
                            }
                        }
                    }
                    push(name, value);
                }
                _ => {}
            }
        }
        let query = pairs
            .iter()
            .map(|(k, v)| {
                format!(
                    "{}={}",
                    crate::util::url_encode_component(k),
                    crate::util::url_encode_component(v)
                )
            })
            .collect::<Vec<_>>()
            .join("&");
        let _ = method;
        if query.is_empty() {
            return Some(action_url);
        }
        if action_url.contains('?') {
            Some(format!("{}&{}", action_url, query))
        } else {
            Some(format!("{}?{}", action_url, query))
        }
    }

    pub fn link_href(&self, node: usize) -> Option<String> {''')
open(p, 'w', encoding='utf-8').write(s)

p = 'src/js.rs'
s = open(p, encoding='utf-8').read()
s = s.replace('''    pub changed: Rc<Cell<bool>>,''', '''    pub pending_nav: Rc<RefCell<Option<String>>>,
    pub changed: Rc<Cell<bool>>,''')
s = s.replace('''            url: Rc::new(RefCell::new(base_url.to_string())),
            changed: Rc::new(Cell::new(false)),''', '''            url: Rc::new(RefCell::new(base_url.to_string())),
            pending_nav: Rc::new(RefCell::new(None)),
            changed: Rc::new(Cell::new(false)),''')
# location href setter
s = s.replace('''    let set = mk_set(ctx, move |v, ctx| {
        let href = val_str(&v, ctx);
        *s.url.borrow_mut() = href;
        s.changed.set(true);
        let _ = ctx;
        Ok(())
    });
    init.accessor(js_string!("href"), Some(get), Some(set), Attribute::all());''', '''    let set = mk_set(ctx, move |v, ctx| {
        let href = val_str(&v, ctx);
        *s.url.borrow_mut() = href.clone();
        *s.pending_nav.borrow_mut() = Some(href);
        s.changed.set(true);
        let _ = ctx;
        Ok(())
    });
    init.accessor(js_string!("href"), Some(get), Some(set), Attribute::all());''')
# assign
s = s.replace('''        mk_method(move |args, ctx| {
            let href = arg_str(args, 0, ctx);
            *s.url.borrow_mut() = href;
            s.changed.set(true);
            Ok(JsValue::undefined())
        }),
        js_string!("assign"),''', '''        mk_method(move |args, ctx| {
            let href = arg_str(args, 0, ctx);
            *s.url.borrow_mut() = href.clone();
            *s.pending_nav.borrow_mut() = Some(href);
            s.changed.set(true);
            Ok(JsValue::undefined())
        }),
        js_string!("assign"),''')
# document.location setter in wrap_document
s = s.replace('''            Some(mk_set(ctx, move |v, ctx| {
                let href = val_str(&v, ctx);
                *s.url.borrow_mut() = href;
                s.changed.set(true);
                Ok(())
            })),
            Attribute::all(),
        );
        init.function(
            mk_method(move |_args, _ctx| Ok(JsValue::undefined())),
            js_string!("close"),''', '''            Some(mk_set(ctx, move |v, ctx| {
                let href = val_str(&v, ctx);
                *s.url.borrow_mut() = href.clone();
                *s.pending_nav.borrow_mut() = Some(href);
                s.changed.set(true);
                Ok(())
            })),
            Attribute::all(),
        );
        init.function(
            mk_method(move |_args, _ctx| Ok(JsValue::undefined())),
            js_string!("close"),''')
# public timer + nav accessors
s = s.replace('''    pub fn take_changed(&self) -> bool {
        let c = self.shared.changed.get();
        self.shared.changed.set(false);
        c
    }
}''', '''    pub fn take_changed(&self) -> bool {
        let c = self.shared.changed.get();
        self.shared.changed.set(false);
        c
    }

    pub fn take_pending_navigation(&mut self) -> Option<String> {
        self.shared.pending_nav.borrow_mut().take()
    }

    pub fn tick_timers(&mut self) {
        let pending = !self.shared.timers.borrow().is_empty();
        if pending {
            self.run_timers();
            self.ctx.run_jobs();
        }
    }
}''')
open(p, 'w', encoding='utf-8').write(s)
print('forms/js nav/timers patched')
