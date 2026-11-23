p = 'src/browser.rs'
s = open(p, encoding='utf-8').read()

s = s.replace('''            render_page(
                &mut pix,
                &page.layout,
                &page.data.doc,
                &mut self.fonts,
                &page.data.images,
                self.scroll,
                CHROME_H,
            );''', '''            {
                let doc = page.dom.borrow();
                render_page(
                    &mut pix,
                    &page.layout,
                    &doc,
                    &mut self.fonts,
                    &page.data.images,
                    self.scroll,
                    CHROME_H,
                );
            }''')

s = s.replace('''        while let Some(n) = cur {
            if let Some(t) = page.data.doc.tag(n) {
                if tags.contains(&t) {
                    return Some(n);
                }
            }
            cur = page.data.doc.nodes[n].parent;
        }''', '''        let doc = page.dom.borrow();
        while let Some(n) = cur {
            if let Some(t) = doc.tag(n) {
                if tags.contains(&t) {
                    return Some(n);
                }
            }
            cur = doc.nodes[n].parent;
        }''')

s = s.replace('''                    if find_link_node(&page.data.doc, node).is_some() {''',
              '''                    if find_link_node(&page.dom.borrow(), node).is_some() {''')

s = s.replace('''        if let Some(page) = &self.page {
            if let Some(link) = find_link_node(&page.data.doc, node) {
                if let Some(href) = page.data.link_href(link) {''', '''        if let Some(page) = &self.page {
            if let Some(link) = find_link_node(&page.dom.borrow(), node) {
                if let Some(href) = page.link_href(link) {''')

s = s.replace('''                .and_then(|p| p.data.doc.attr(input, "type").map(|t| t.to_ascii_lowercase()))''',
              '''                .and_then(|p| p.dom.borrow().attr(input, "type").map(|t| t.to_ascii_lowercase()))''')

s = s.replace('''        let target = self
            .page
            .as_ref()
            .and_then(|p| p.data.doc.find_by_id(id));''', '''        let target = self
            .page
            .as_ref()
            .and_then(|p| p.dom.borrow().find_by_id(id));''')

s = s.replace('''            .and_then(|p| p.data.doc.attr(node, "value").map(|s| s.to_string()))''',
              '''            .and_then(|p| p.dom.borrow().attr(node, "value").map(|s| s.to_string()))''')

s = s.replace('''        let checked = self
            .page
            .as_ref()
            .and_then(|p| p.data.doc.attr(node, "checked").map(|_| true))
            .unwrap_or(false);
        if let Some(page) = &mut self.page {
            if checked {
                page.data.doc.remove_attr(node, "checked");
            } else {
                page.data.doc.set_attr(node, "checked", "");
            }''', '''        let checked = self
            .page
            .as_ref()
            .and_then(|p| p.dom.borrow().attr(node, "checked").map(|_| true))
            .unwrap_or(false);
        if let Some(page) = &mut self.page {
            {
                let mut doc = page.dom.borrow_mut();
                if checked {
                    doc.remove_attr(node, "checked");
                } else {
                    doc.set_attr(node, "checked", "");
                }
            }''')

s = s.replace('''        let options: Vec<(NodeId, String)> = {
            let page = match &self.page {
                Some(p) => p,
                None => return,
            };
            page.data.doc.nodes[node]
                .children
                .iter()
                .filter(|&&c| page.data.doc.tag(c) == Some("option"))
                .map(|&c| {
                    let text = page.data.doc.text_content(c).trim().to_string();
                    (c, text)
                })
                .collect()
        };
        if options.is_empty() {
            return;
        }
        if let Some(page) = &mut self.page {
            let mut current = 0;
            for (i, (c, _)) in options.iter().enumerate() {
                if page.data.doc.attr(*c, "selected").is_some() {
                    current = i;
                }
            }
            let next = (current + 1) % options.len();
            for (i, (c, _)) in options.iter().enumerate() {
                if i == next {
                    page.data.doc.set_attr(*c, "selected", "");
                } else {
                    page.data.doc.remove_attr(*c, "selected");
                }
            }''', '''        let options: Vec<(NodeId, String)> = {
            let page = match &self.page {
                Some(p) => p,
                None => return,
            };
            let doc = page.dom.borrow();
            doc.nodes[node]
                .children
                .iter()
                .filter(|&&c| doc.tag(c) == Some("option"))
                .map(|&c| {
                    let text = doc.text_content(c).trim().to_string();
                    (c, text)
                })
                .collect()
        };
        if options.is_empty() {
            return;
        }
        if let Some(page) = &mut self.page {
            {
                let mut doc = page.dom.borrow_mut();
                let mut current = 0;
                for (i, (c, _)) in options.iter().enumerate() {
                    if doc.attr(*c, "selected").is_some() {
                        current = i;
                    }
                }
                let next = (current + 1) % options.len();
                for (i, (c, _)) in options.iter().enumerate() {
                    if i == next {
                        doc.set_attr(*c, "selected", "");
                    } else {
                        doc.remove_attr(*c, "selected");
                    }
                }
            }''')

s = s.replace('''        let is_textarea = self
            .page
            .as_ref()
            .and_then(|p| p.data.doc.tag(node))
            .map(|t| t == "textarea")
            .unwrap_or(false);
        if let Some(page) = &mut self.page {
            if is_textarea {
                let mut value = page.data.doc.text_content(node);
                self.input_cursor = self.input_cursor.min(value.chars().count());
                let mut chars: Vec<char> = value.chars().collect();
                let mut insert_at = self.input_cursor.min(chars.len());
                for c in text.chars() {
                    if c == '\\r' {
                        continue;
                    }
                    chars.insert(insert_at, c);
                    insert_at += 1;
                }
                value = chars.into_iter().collect();
                page.data.doc.set_text_content(node, &value);
                self.input_cursor = insert_at;
            } else {
                let mut value = page
                    .data
                    .doc
                    .attr(node, "value")
                    .unwrap_or("")
                    .to_string();
                let mut chars: Vec<char> = value.chars().collect();
                self.input_cursor = self.input_cursor.min(chars.len());
                let mut insert_at = self.input_cursor;
                for c in text.chars() {
                    if c == '\\r' || c == '\\n' {
                        continue;
                    }
                    chars.insert(insert_at, c);
                    insert_at += 1;
                }
                value = chars.into_iter().collect();
                page.data.doc.set_attr(node, "value", &value);
                self.input_cursor = insert_at;
            }''', '''        let is_textarea = self
            .page
            .as_ref()
            .and_then(|p| p.dom.borrow().tag(node).map(|t| t == "textarea"))
            .unwrap_or(false);
        if let Some(page) = &mut self.page {
            {
                let mut doc = page.dom.borrow_mut();
                if is_textarea {
                    let value = doc.text_content(node);
                    self.input_cursor = self.input_cursor.min(value.chars().count());
                    let mut chars: Vec<char> = value.chars().collect();
                    let mut insert_at = self.input_cursor.min(chars.len());
                    for c in text.chars() {
                        if c == '\\r' {
                            continue;
                        }
                        chars.insert(insert_at, c);
                        insert_at += 1;
                    }
                    let new_value: String = chars.into_iter().collect();
                    doc.set_text_content(node, &new_value);
                    self.input_cursor = insert_at;
                } else {
                    let value = doc.attr(node, "value").unwrap_or("").to_string();
                    let mut chars: Vec<char> = value.chars().collect();
                    self.input_cursor = self.input_cursor.min(chars.len());
                    let mut insert_at = self.input_cursor;
                    for c in text.chars() {
                        if c == '\\r' || c == '\\n' {
                            continue;
                        }
                        chars.insert(insert_at, c);
                        insert_at += 1;
                    }
                    let new_value: String = chars.into_iter().collect();
                    doc.set_attr(node, "value", &new_value);
                    self.input_cursor = insert_at;
                }
            }''')

s = s.replace('''        let is_textarea = self
            .page
            .as_ref()
            .and_then(|p| p.data.doc.tag(node))
            .map(|t| t == "textarea")
            .unwrap_or(false);
        if let Some(page) = &mut self.page {
            if is_textarea {
                let value = page.data.doc.text_content(node);
                let mut chars: Vec<char> = value.chars().collect();
                if !chars.is_empty() {
                    chars.pop();
                }
                page.data.doc.set_text_content(node, &chars.into_iter().collect::<String>());
                self.input_cursor = chars_len(&page.data.doc.text_content(node));
            } else {
                let value = page.data.doc.attr(node, "value").unwrap_or("").to_string();
                let mut chars: Vec<char> = value.chars().collect();
                if !chars.is_empty() {
                    chars.pop();
                }
                let value: String = chars.into_iter().collect();
                page.data.doc.set_attr(node, "value", &value);
                self.input_cursor = value.chars().count();
            }''', '''        let is_textarea = self
            .page
            .as_ref()
            .and_then(|p| p.dom.borrow().tag(node).map(|t| t == "textarea"))
            .unwrap_or(false);
        if let Some(page) = &mut self.page {
            {
                let mut doc = page.dom.borrow_mut();
                if is_textarea {
                    let value = doc.text_content(node);
                    let mut chars: Vec<char> = value.chars().collect();
                    if !chars.is_empty() {
                        chars.pop();
                    }
                    let new_value: String = chars.into_iter().collect();
                    doc.set_text_content(node, &new_value);
                    self.input_cursor = new_value.chars().count();
                } else {
                    let value = doc.attr(node, "value").unwrap_or("").to_string();
                    let mut chars: Vec<char> = value.chars().collect();
                    if !chars.is_empty() {
                        chars.pop();
                    }
                    let new_value: String = chars.into_iter().collect();
                    doc.set_attr(node, "value", &new_value);
                    self.input_cursor = new_value.chars().count();
                }
            }''')

s = s.replace('''                let t = if page.data.title.is_empty() {
                    page.data.url.clone()
                } else {
                    page.data.title.clone()
                };''', '''                let t = if page.title().is_empty() {
                    page.data.url.clone()
                } else {
                    page.title()
                };''')
open(p, 'w', encoding='utf-8').write(s)
print('browser patched')
