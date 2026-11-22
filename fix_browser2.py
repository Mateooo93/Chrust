p = 'src/browser.rs'
s = open(p, encoding='utf-8').read()

# click_page: pending nav after dispatch, form submission defaults
s = s.replace('''        if prevented {
            self.set_title(false);
            self.request_redraw();
            return;
        }
        // Links.''', '''        if prevented {
            self.set_title(false);
            self.request_redraw();
            return;
        }
        let pending = self
            .page
            .as_mut()
            .and_then(|p| p.take_pending_navigation());
        if let Some(url) = pending {
            self.navigate(&url, true);
            return;
        }
        // Submit buttons / inputs.
        let submit = {
            let page = match &self.page {
                Some(p) => p,
                None => return,
            };
            let doc = page.dom.borrow();
            let mut form = None;
            if doc.tag(node) == Some("button") {
                let ty = doc.attr(node, "type").unwrap_or("submit").to_ascii_lowercase();
                if ty == "submit" {
                    form = page.ancestor_tag(node, "form");
                }
            } else if doc.tag(node) == Some("input") {
                let ty = doc.attr(node, "type").unwrap_or("text").to_ascii_lowercase();
                if ty == "submit" || ty == "image" {
                    form = page.ancestor_tag(node, "form");
                }
            }
            form.and_then(|f| page.form_submit_url(f))
        };
        if let Some(url) = submit {
            self.navigate(&url, true);
            return;
        }
        // Links.''')

# Enter in focused input submits its form
s = s.replace('''        if self.focused_input.is_some() {
            match key {
                Key::Named(NamedKey::Enter) => {
                    self.blur_input();
                }''', '''        if self.focused_input.is_some() {
            match key {
                Key::Named(NamedKey::Enter) => {
                    let submit = self.focused_input.and_then(|node| {
                        let page = self.page.as_ref()?;
                        let form = page.ancestor_tag(node, "form")?;
                        page.form_submit_url(form)
                    });
                    if let Some(url) = submit {
                        self.navigate(&url, true);
                    } else {
                        self.blur_input();
                    }
                }''')

# on_loaded pending nav from scripts, and timers scheduling
s = s.replace('''                self.status = if title.is_empty() {
                    String::new()
                } else {
                    format!("Loaded — {}", title)
                };''', '''                self.status = if title.is_empty() {
                    String::new()
                } else {
                    format!("Loaded — {}", title)
                };
                self.next_js_tick = Instant::now() + Duration::from_millis(250);''')

# add field next_js_tick
s = s.replace('''    last_frame: Instant,
    loading_started: Instant,
}''', '''    last_frame: Instant,
    loading_started: Instant,
    next_js_tick: Instant,
}''')
s = s.replace('''        last_frame: Instant::now(),
        loading_started: Instant::now(),
    };''', '''        last_frame: Instant::now(),
        loading_started: Instant::now(),
        next_js_tick: Instant::now(),
    };''')

# tick timers in about_to_wait
s = s.replace('''    fn about_to_wait(&mut self, event_loop: &ActiveEventLoop) {
        if self.loading {
            if let Some(win) = &self.window {
                win.request_redraw();
            }
            event_loop.set_control_flow(ControlFlow::WaitUntil(
                Instant::now() + Duration::from_millis(80),
            ));
        } else {
            event_loop.set_control_flow(ControlFlow::Wait);
        }
    }''', '''    fn about_to_wait(&mut self, event_loop: &ActiveEventLoop) {
        if self.loading {
            if let Some(win) = &self.window {
                win.request_redraw();
            }
            event_loop.set_control_flow(ControlFlow::WaitUntil(
                Instant::now() + Duration::from_millis(80),
            ));
            return;
        }
        let has_timers = self.page.as_ref().map(|p| p.has_js_timers()).unwrap_or(false);
        if has_timers && Instant::now() >= self.next_js_tick {
            self.next_js_tick = Instant::now() + Duration::from_millis(250);
            let changed = {
                let (page_opt, fonts) = (&mut self.page, &mut self.fonts);
                if let Some(page) = page_opt {
                    page.js_tick(fonts)
                } else {
                    false
                }
            };
            if changed {
                self.set_title(false);
                self.request_redraw();
            }
            if self.page.as_ref().map(|p| p.has_js_timers()).unwrap_or(false) {
                event_loop.set_control_flow(ControlFlow::WaitUntil(self.next_js_tick));
                return;
            }
        }
        event_loop.set_control_flow(ControlFlow::Wait);
    }''')
open(p, 'w', encoding='utf-8').write(s)
print('browser forms/nav/timers patched')
