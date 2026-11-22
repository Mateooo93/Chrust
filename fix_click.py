p = 'src/page.rs'
s = open(p, encoding='utf-8').read()
s = s.replace('''    pub fn title(&self) -> String {''', '''    pub fn query_selector(&self, selector: &str) -> Option<usize> {
        let doc = self.dom.borrow();
        let mut sels = Vec::new();
        for part in crate::css::split_top_level(selector, ',') {
            if let Some(sel) = crate::css::parse_selector(&part) {
                sels.push(sel);
            }
        }
        if sels.is_empty() {
            return None;
        }
        let ctx = crate::css::MatchContext {
            doc: &doc,
            hovered: None,
            active: None,
            focused: None,
        };
        doc.walk(doc.root)
            .into_iter()
            .find(|&n| doc.is_element(n) && sels.iter().any(|s| s.matches(n, &ctx)))
    }

    pub fn title(&self) -> String {''')
open(p, 'w', encoding='utf-8').write(s)

p = 'src/main.rs'
s = open(p, encoding='utf-8').read()
s = s.replace('''    dump_layout: bool,
}''', '''    dump_layout: bool,
    click: Option<String>,
}''')
s = s.replace('''        dump_layout: false,
    };''', '''        dump_layout: false,
        click: None,
    };''')
s = s.replace('''            "--dump-layout" => opts.dump_layout = true,''', '''            "--dump-layout" => opts.dump_layout = true,
            "--click" => {
                i += 1;
                if i < args.len() {
                    opts.click = Some(args[i].clone());
                }
            }''')
s = s.replace('''        let page = Page::new(data, &mut fonts, viewport);
        if opts.dump_text {''', '''        let mut page = Page::new(data, &mut fonts, viewport);
        if let Some(selector) = &opts.click {
            if let Some(node) = page.query_selector(selector) {
                page.dispatch_js(Some(node), "click");
                if page.take_js_changed() {
                    page.relayout(&mut fonts, viewport);
                }
            } else {
                eprintln!("click: no element matched {}", selector);
            }
        }
        if opts.dump_text {''')
open(p, 'w', encoding='utf-8').write(s)
print('click option added')
