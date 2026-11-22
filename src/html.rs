#![allow(dead_code)]
//! A hand-written HTML tokenizer and tree builder (HTML5-flavoured, with
//! error recovery for the common cases found on real pages).

use crate::dom::{is_void_tag, Document, NodeData, NodeId};

#[derive(Debug, Clone, PartialEq)]
pub enum Token {
    StartTag { name: String, attrs: Vec<(String, String)>, self_closing: bool },
    EndTag { name: String },
    Text(String),
    Comment(String),
    Doctype(String),
    Eof,
}

struct RawMode {
    tag: String,
    decode: bool,
}

pub struct Tokenizer {
    chars: Vec<char>,
    pos: usize,
    raw: Option<RawMode>,
}

impl Tokenizer {
    pub fn new(input: &str) -> Tokenizer {
        let normalized = input.replace("\r\n", "\n").replace('\r', "\n");
        Tokenizer { chars: normalized.chars().collect(), pos: 0, raw: None }
    }

    fn peek(&self) -> Option<char> {
        self.chars.get(self.pos).copied()
    }

    fn starts_with_ci(&self, s: &str) -> bool {
        let n = s.chars().count();
        if self.pos + n > self.chars.len() {
            return false;
        }
        let mut ok = true;
        for (i, c) in s.chars().enumerate() {
            if !self.chars[self.pos + i].eq_ignore_ascii_case(&c) {
                ok = false;
                break;
            }
        }
        ok
    }

    pub fn next_token(&mut self) -> Token {
        loop {
            if self.pos >= self.chars.len() {
                if self.raw.is_some() {
                    self.raw = None;
                    continue;
                }
                return Token::Eof;
            }
            if let Some(raw) = &self.raw {
                let tag = raw.tag.clone();
                let decode = raw.decode;
                // Find "</tag" followed by whitespace, '/', or '>'.
                let needle = format!("</{}", tag);
                let mut found: Option<usize> = None;
                let mut i = self.pos;
                while i < self.chars.len() {
                    if self.chars[i] == '<' && self.matches_at(i, &needle) {
                        let after = i + needle.chars().count();
                        if after >= self.chars.len()
                            || self.chars[after].is_whitespace()
                            || self.chars[after] == '>'
                            || self.chars[after] == '/'
                        {
                            found = Some(i);
                            break;
                        }
                    }
                    i += 1;
                }
                match found {
                    Some(idx) => {
                        self.raw = None;
                        let text: String = self.chars[self.pos..idx].iter().collect();
                        self.pos = idx;
                        if !text.is_empty() {
                            let text = if decode { decode_entities(&text) } else { text };
                            return Token::Text(text);
                        }
                        // fall through to parse the end tag
                    }
                    None => {
                        self.raw = None;
                        let text: String = self.chars[self.pos..].iter().collect();
                        self.pos = self.chars.len();
                        if !text.is_empty() {
                            let text = if decode { decode_entities(&text) } else { text };
                            return Token::Text(text);
                        }
                        continue;
                    }
                }
            }

            let c = self.peek().unwrap();
            if c == '<' {
                if self.starts_with_at("<!--") {
                    return self.parse_comment();
                }
                if self.starts_with_ci("<!doctype") {
                    return self.parse_doctype();
                }
                if self.starts_with_at("<!") {
                    return self.parse_bogus_comment();
                }
                if self.starts_with_at("<?") {
                    return self.parse_bogus_comment();
                }
                if self.starts_with_at("</") {
                    // End tag if followed by a name char, otherwise treat as text.
                    if let Some(nc) = self.chars.get(self.pos + 2).copied() {
                        if nc.is_ascii_alphabetic() {
                            return self.parse_end_tag();
                        }
                    }
                    self.pos += 1;
                    return Token::Text("<".to_string());
                }
                if let Some(nc) = self.chars.get(self.pos + 1).copied() {
                    if nc.is_ascii_alphabetic() {
                        return self.parse_start_tag();
                    }
                }
                self.pos += 1;
                return Token::Text("<".to_string());
            }

            // Data text until '<' or entity.
            let start = self.pos;
            let mut text = String::new();
            while self.pos < self.chars.len() {
                let ch = self.chars[self.pos];
                if ch == '<' {
                    break;
                }
                if ch == '&' {
                    if let Some(decoded) = self.decode_entity() {
                        text.push_str(&decoded);
                        continue;
                    }
                }
                text.push(ch);
                self.pos += 1;
            }
            if self.pos == start {
                // Defensive: guarantee progress.
                text.push(self.chars[self.pos]);
                self.pos += 1;
            }
            return Token::Text(text);
        }
    }

    fn matches_at(&self, pos: usize, s: &str) -> bool {
        let mut i = pos;
        for c in s.chars() {
            if i >= self.chars.len() || !self.chars[i].eq_ignore_ascii_case(&c) {
                return false;
            }
            i += 1;
        }
        true
    }

    fn starts_with_at(&self, s: &str) -> bool {
        self.matches_at(self.pos, s) || (s == "<!--" && self.matches_at(self.pos, "<!--"))
    }

    fn parse_comment(&mut self) -> Token {
        self.pos += 4;
        let start = self.pos;
        while self.pos < self.chars.len() {
            if self.chars[self.pos] == '-' && self.matches_at(self.pos, "-->") {
                let text: String = self.chars[start..self.pos].iter().collect();
                self.pos += 3;
                return Token::Comment(text);
            }
            self.pos += 1;
        }
        let text: String = self.chars[start..].iter().collect();
        Token::Comment(text)
    }

    fn parse_bogus_comment(&mut self) -> Token {
        while self.pos < self.chars.len() && self.chars[self.pos] != '>' {
            self.pos += 1;
        }
        if self.pos < self.chars.len() {
            self.pos += 1;
        }
        Token::Comment(String::new())
    }

    fn parse_doctype(&mut self) -> Token {
        self.pos += 9; // <!doctype
        let start = self.pos;
        while self.pos < self.chars.len() && self.chars[self.pos] != '>' {
            self.pos += 1;
        }
        let text: String = self.chars[start..self.pos].iter().collect();
        if self.pos < self.chars.len() {
            self.pos += 1;
        }
        Token::Doctype(text.trim().to_string())
    }

    fn parse_end_tag(&mut self) -> Token {
        self.pos += 2;
        while let Some(c) = self.peek() {
            if c.is_whitespace() {
                self.pos += 1;
            } else {
                break;
            }
        }
        let mut name = String::new();
        while let Some(c) = self.peek() {
            if c.is_whitespace() || c == '>' || c == '/' {
                break;
            }
            name.push(c.to_ascii_lowercase());
            self.pos += 1;
        }
        while let Some(c) = self.peek() {
            self.pos += 1;
            if c == '>' {
                break;
            }
        }
        Token::EndTag { name }
    }

    fn parse_start_tag(&mut self) -> Token {
        self.pos += 1;
        let mut name = String::new();
        while let Some(c) = self.peek() {
            if c.is_alphanumeric() || c == '-' || c == ':' || c == '_' || c == '.' {
                name.push(c.to_ascii_lowercase());
                self.pos += 1;
            } else {
                break;
            }
        }
        let mut attrs: Vec<(String, String)> = Vec::new();
        let mut self_closing = false;
        loop {
            while let Some(c) = self.peek() {
                if c.is_whitespace() {
                    self.pos += 1;
                } else {
                    break;
                }
            }
            let c = match self.peek() {
                Some(c) => c,
                None => break,
            };
            if c == '>' {
                self.pos += 1;
                break;
            }
            if c == '/' {
                if self.chars.get(self.pos + 1) == Some(&'>') {
                    self_closing = true;
                    self.pos += 2;
                    break;
                }
                self.pos += 1;
                continue;
            }
            let mut aname = String::new();
            while let Some(ch) = self.peek() {
                if ch.is_whitespace() || ch == '=' || ch == '>' || ch == '/' {
                    break;
                }
                aname.push(ch.to_ascii_lowercase());
                self.pos += 1;
            }
            if aname.is_empty() {
                self.pos += 1;
                continue;
            }
            while let Some(ch) = self.peek() {
                if ch.is_whitespace() {
                    self.pos += 1;
                } else {
                    break;
                }
            }
            let mut value = String::new();
            if self.peek() == Some('=') {
                self.pos += 1;
                while let Some(ch) = self.peek() {
                    if ch.is_whitespace() {
                        self.pos += 1;
                    } else {
                        break;
                    }
                }
                match self.peek() {
                    Some('"') | Some('\'') => {
                        let quote = self.peek().unwrap();
                        self.pos += 1;
                        while let Some(ch) = self.peek() {
                            if ch == quote {
                                self.pos += 1;
                                break;
                            }
                            if ch == '&' {
                                if let Some(dec) = self.decode_entity() {
                                    value.push_str(&dec);
                                    continue;
                                }
                            }
                            value.push(ch);
                            self.pos += 1;
                        }
                    }
                    _ => {
                        while let Some(ch) = self.peek() {
                            if ch.is_whitespace() || ch == '>' {
                                break;
                            }
                            if ch == '&' {
                                if let Some(dec) = self.decode_entity() {
                                    value.push_str(&dec);
                                    continue;
                                }
                            }
                            value.push(ch);
                            self.pos += 1;
                        }
                    }
                }
            }
            if !attrs.iter().any(|(n, _)| n == &aname) {
                attrs.push((aname, value));
            }
        }

        if !self_closing {
            match name.as_str() {
                "script" | "style" | "xmp" | "iframe" | "noembed" | "noframes" => {
                    self.raw = Some(RawMode { tag: name.clone(), decode: false });
                }
                "title" | "textarea" => {
                    self.raw = Some(RawMode { tag: name.clone(), decode: true });
                }
                "plaintext" => {
                    self.raw = Some(RawMode { tag: "\u{0}plaintext".to_string(), decode: false });
                }
                _ => {}
            }
        }

        Token::StartTag { name, attrs, self_closing }
    }

    fn decode_entity(&mut self) -> Option<String> {
        // self.pos points at '&'
        let start = self.pos;
        let mut end = start + 1;
        let limit = (start + 40).min(self.chars.len());
        while end < limit {
            let c = self.chars[end];
            if c == ';' {
                break;
            }
            if c.is_whitespace() || c == '<' || c == '&' {
                return None;
            }
            end += 1;
        }
        if end >= self.chars.len() || self.chars[end] != ';' {
            return None;
        }
        let name: String = self.chars[start + 1..end].iter().collect();
        let decoded = if let Some(num) = name.strip_prefix('#') {
            let cp = if let Some(hex) = num.strip_prefix('x').or_else(|| num.strip_prefix('X')) {
                u32::from_str_radix(hex, 16).ok()?
            } else {
                num.parse::<u32>().ok()?
            };
            let cp = if cp == 0 || cp > 0x10FFFF { 0xFFFD } else { cp };
            char::from_u32(cp).map(|c| c.to_string())?
        } else {
            named_entity(&name)?
        };
        self.pos = end + 1;
        Some(decoded)
    }
}

pub fn decode_entities(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    let chars: Vec<char> = s.chars().collect();
    let mut i = 0;
    while i < chars.len() {
        if chars[i] == '&' {
            let mut j = i + 1;
            let limit = (i + 40).min(chars.len());
            while j < limit && chars[j] != ';' && !chars[j].is_whitespace() && chars[j] != '<' && chars[j] != '&' {
                j += 1;
            }
            if j < chars.len() && chars[j] == ';' {
                let name: String = chars[i + 1..j].iter().collect();
                let decoded = if let Some(num) = name.strip_prefix('#') {
                    let cp = if let Some(hex) = num.strip_prefix('x').or_else(|| num.strip_prefix('X')) {
                        u32::from_str_radix(hex, 16).ok()
                    } else {
                        num.parse::<u32>().ok()
                    };
                    cp.and_then(|cp| char::from_u32(if cp == 0 || cp > 0x10FFFF { 0xFFFD } else { cp }))
                        .map(|c| c.to_string())
                } else {
                    named_entity(&name)
                };
                if let Some(d) = decoded {
                    out.push_str(&d);
                    i = j + 1;
                    continue;
                }
            }
        }
        out.push(chars[i]);
        i += 1;
    }
    out
}

pub fn named_entity(name: &str) -> Option<String> {
    let s = match name {
        "amp" | "AMP" => "&",
        "lt" | "LT" => "<",
        "gt" | "GT" => ">",
        "quot" | "QUOT" => "\"",
        "apos" => "'",
        "nbsp" => "\u{00A0}",
        "copy" => "©",
        "reg" => "®",
        "trade" => "™",
        "hellip" => "…",
        "mdash" => "—",
        "ndash" => "–",
        "lsquo" => "‘",
        "rsquo" => "’",
        "ldquo" => "“",
        "rdquo" => "”",
        "laquo" => "«",
        "raquo" => "»",
        "middot" => "·",
        "bull" => "•",
        "dagger" => "†",
        "Dagger" => "‡",
        "times" => "×",
        "divide" => "÷",
        "deg" => "°",
        "plusmn" => "±",
        "sup1" => "¹",
        "sup2" => "²",
        "sup3" => "³",
        "frac14" => "¼",
        "frac12" => "½",
        "frac34" => "¾",
        "euro" => "€",
        "pound" => "£",
        "yen" => "¥",
        "cent" => "¢",
        "sect" => "§",
        "para" => "¶",
        "micro" => "µ",
        "alpha" => "α",
        "beta" => "β",
        "gamma" => "γ",
        "delta" => "δ",
        "pi" => "π",
        "sigma" => "σ",
        "omega" => "ω",
        "Omega" => "Ω",
        "infin" => "∞",
        "ne" => "≠",
        "le" => "≤",
        "ge" => "≥",
        "larr" => "←",
        "rarr" => "→",
        "uarr" => "↑",
        "darr" => "↓",
        "harr" => "↔",
        "check" => "✓",
        "hearts" => "♥",
        "spades" => "♠",
        "clubs" => "♣",
        "diams" => "♦",
        "star" => "☆",
        "starf" => "★",
        "ensp" => "\u{2002}",
        "emsp" => "\u{2003}",
        "thinsp" => "\u{2009}",
        "zwnj" => "\u{200C}",
        "zwj" => "\u{200D}",
        "shy" => "\u{00AD}",
        _ => return None,
    };
    Some(s.to_string())
}

const P_CLOSERS: &[&str] = &[
    "address", "article", "aside", "blockquote", "details", "dialog", "div", "dl",
    "fieldset", "figcaption", "figure", "footer", "form", "h1", "h2", "h3", "h4", "h5",
    "h6", "header", "hgroup", "hr", "main", "menu", "nav", "ol", "p", "pre", "section",
    "table", "ul", "li", "dd", "dt",
];

const HEAD_TAGS: &[&str] = &["title", "base", "link", "meta", "style", "script", "noscript", "template"];

pub struct TreeBuilder {
    pub doc: Document,
    stack: Vec<NodeId>,
    html: Option<NodeId>,
    head: Option<NodeId>,
    body: Option<NodeId>,
    seen_content: bool,
}

impl TreeBuilder {
    pub fn new() -> TreeBuilder {
        let mut doc = Document::new();
        let html = doc.create_element("html");
        let root = doc.root;
        doc.append_child(root, html);
        TreeBuilder {
            doc,
            stack: vec![html],
            html: Some(html),
            head: None,
            body: None,
            seen_content: false,
        }
    }

    fn current(&self) -> NodeId {
        *self.stack.last().unwrap_or(&self.doc.root)
    }

    fn ensure_head(&mut self) -> NodeId {
        if let Some(h) = self.head {
            return h;
        }
        let head = self.doc.create_element("head");
        let html = self.html.unwrap();
        match self.body {
            Some(body) => self.doc.insert_before(html, head, Some(body)),
            None => self.doc.append_child(html, head),
        }
        self.head = Some(head);
        head
    }

    fn ensure_body(&mut self) -> NodeId {
        if let Some(b) = self.body {
            return b;
        }
        let body = self.doc.create_element("body");
        let html = self.html.unwrap();
        self.doc.append_child(html, body);
        // Close the head if it is open.
        if let Some(pos) = self.stack.iter().position(|&n| Some(n) == self.head) {
            self.stack.truncate(pos + 1);
        }
        if !self.stack.contains(&html) {
            self.stack.clear();
            self.stack.push(html);
        }
        self.stack.push(body);
        self.body = Some(body);
        body
    }

    fn merge_attrs(&mut self, node: NodeId, attrs: &[(String, String)]) {
        for (n, v) in attrs {
            self.doc.set_attr(node, n, v);
        }
    }

    fn attach(&mut self, node: NodeId) {
        let parent = self.current();
        self.doc.append_child(parent, node);
    }

    fn append_text(&mut self, text: &str) {
        if text.is_empty() {
            return;
        }
        let parent = self.current();
        if let Some(&last) = self.doc.nodes[parent].children.last() {
            if let NodeData::Text(t) = &mut self.doc.nodes[last].data {
                t.push_str(text);
                return;
            }
        }
        let node = self.doc.create_text(text);
        self.doc.append_child(parent, node);
    }

    fn close_through(&mut self, targets: &[&str]) -> bool {
        let mut i = self.stack.len();
        while i > 0 {
            i -= 1;
            let tag = self.doc.tag(self.stack[i]).unwrap_or("");
            if targets.contains(&tag) {
                self.stack.truncate(i);
                return true;
            }
        }
        false
    }

    fn close_through_stop(&mut self, targets: &[&str], stop: &[&str]) {
        let mut i = self.stack.len();
        while i > 0 {
            i -= 1;
            let tag = self.doc.tag(self.stack[i]).unwrap_or("");
            if targets.contains(&tag) {
                self.stack.truncate(i);
                return;
            }
            if stop.contains(&tag) {
                return;
            }
        }
    }

    fn auto_close(&mut self, tag: &str) {
        match tag {
            "li" => {
                self.close_through_stop(&["li"], &["ul", "ol", "menu", "body"]);
            }
            "dt" | "dd" => {
                self.close_through_stop(&["dt", "dd"], &["dl", "body"]);
            }
            "option" => {
                let top = self.current();
                if self.doc.tag(top) == Some("option") {
                    self.stack.pop();
                }
            }
            "optgroup" => {
                let top = self.current();
                if self.doc.tag(top) == Some("option") {
                    self.stack.pop();
                }
                if self.doc.tag(self.current()) == Some("optgroup") {
                    self.stack.pop();
                }
            }
            "tr" => {
                self.close_through_stop(&["td", "th"], &["tr", "table"]);
                self.close_through_stop(&["tr"], &["table"]);
            }
            "td" | "th" => {
                self.close_through_stop(&["td", "th"], &["tr", "table"]);
            }
            "thead" | "tbody" | "tfoot" => {
                self.close_through_stop(&["thead", "tbody", "tfoot", "tr", "td", "th"], &["table"]);
            }
            "colgroup" => {
                self.close_through_stop(&["colgroup"], &["table"]);
            }
            "h1" | "h2" | "h3" | "h4" | "h5" | "h6" => {
                self.close_through_stop(&["h1", "h2", "h3", "h4", "h5", "h6"], &["body"]);
            }
            "a" => {
                self.close_through_stop(&["a"], &["body"]);
            }
            "button" => {
                self.close_through_stop(&["button"], &["body"]);
            }
            "select" => {
                self.close_through_stop(&["option", "optgroup"], &["select", "body"]);
            }
            "p" => {
                self.close_through_stop(&["p"], &["body", "button"]);
            }
            _ => {}
        }
        if P_CLOSERS.contains(&tag) && tag != "li" && tag != "dd" && tag != "dt" {
            self.close_through_stop(&["p"], &["body", "button"]);
        }
    }

    fn start_tag(&mut self, name: &str, attrs: &[(String, String)], _self_closing: bool) {
        match name {
            "html" => {
                if let Some(html) = self.html {
                    self.merge_attrs(html, attrs);
                }
                return;
            }
            "head" => {
                if self.body.is_some() {
                    return;
                }
                let head = self.ensure_head();
                self.merge_attrs(head, attrs);
                if !self.stack.contains(&head) {
                    self.stack.truncate(1);
                    self.stack.push(head);
                }
                return;
            }
            "body" => {
                let body = self.ensure_body();
                self.merge_attrs(body, attrs);
                self.stack.truncate(1);
                self.stack.push(body);
                return;
            }
            _ => {}
        }

        let in_head = self
            .head
            .map(|h| self.stack.contains(&h))
            .unwrap_or(false);

        if self.body.is_none() && (in_head || HEAD_TAGS.contains(&name)) && name != "template" {
            let head = self.ensure_head();
            if !self.stack.contains(&head) {
                self.stack.truncate(1);
                self.stack.push(head);
            }
            // Some tags force the head to end.
            if name == "title" || name == "style" || name == "script" || name == "noscript"
                || name == "template"
            {
                let el = self.doc.create_element(name);
                self.merge_attrs(el, attrs);
                self.attach(el);
                if !is_void_tag(name) {
                    self.stack.push(el);
                }
            } else {
                let el = self.doc.create_element(name);
                self.merge_attrs(el, attrs);
                self.attach(el);
            }
            return;
        }

        if name == "template" && self.body.is_none() {
            let head = self.ensure_head();
            if !self.stack.contains(&head) {
                self.stack.truncate(1);
                self.stack.push(head);
            }
            let el = self.doc.create_element(name);
            self.merge_attrs(el, attrs);
            self.attach(el);
            self.stack.push(el);
            return;
        }

        self.ensure_body();
        self.auto_close(name);
        let el = self.doc.create_element(name);
        self.merge_attrs(el, attrs);
        self.attach(el);
        if !is_void_tag(name) {
            self.stack.push(el);
        }
        self.seen_content = true;
    }

    fn end_tag(&mut self, name: &str) {
        match name {
            "html" | "body" => return,
            "head" => {
                if let Some(pos) = self.stack.iter().position(|&n| Some(n) == self.head) {
                    self.stack.truncate(pos);
                }
                return;
            }
            "br" => {
                self.start_tag("br", &[], false);
                return;
            }
            _ => {}
        }
        if name == "p" {
            // </p> with no open p inserts an empty p.
            if !self.stack.iter().any(|&n| self.doc.tag(n) == Some("p")) {
                self.ensure_body();
                let p = self.doc.create_element("p");
                self.attach(p);
                return;
            }
        }
        let mut i = self.stack.len();
        while i > 0 {
            i -= 1;
            if self.doc.tag(self.stack[i]) == Some(name) {
                self.stack.truncate(i);
                return;
            }
        }
    }

    pub fn process(&mut self, token: Token) {
        match token {
            Token::Doctype(d) => {
                if self.doc.nodes.len() <= 2 {
                    let node = self.doc.add_node(NodeData::Doctype(d));
                    let root = self.doc.root;
                    self.doc.append_child(root, node);
                }
            }
            Token::Comment(c) => {
                let node = self.doc.add_node(NodeData::Comment(c));
                let parent = self.current();
                self.doc.append_child(parent, node);
            }
            Token::Text(t) => {
                if self.body.is_none() {
                    let current_is_headish = self
                        .stack
                        .last()
                        .and_then(|&n| self.doc.tag(n))
                        .map(|t| matches!(t, "title" | "style" | "script" | "noscript" | "template" | "head" | "html"))
                        .unwrap_or(false);
                    if current_is_headish && !self.stack.contains(&self.head.unwrap_or(usize::MAX)) {
                        // text directly under html: only whitespace is ignorable
                        if t.trim().is_empty() {
                            return;
                        }
                        self.ensure_body();
                    } else if current_is_headish && self.stack.contains(&self.head.unwrap_or(usize::MAX)) {
                        // Inside head: keep whitespace, which is harmless.
                        self.append_text(&t);
                        return;
                    } else if t.trim().is_empty() {
                        return;
                    } else {
                        self.ensure_body();
                    }
                }
                self.append_text(&t);
            }
            Token::StartTag { name, attrs, self_closing } => self.start_tag(&name, &attrs, self_closing),
            Token::EndTag { name } => self.end_tag(&name),
            Token::Eof => {}
        }
    }

    pub fn finish(mut self) -> Document {
        self.ensure_body();
        self.doc
    }
}

pub fn parse_document(input: &str) -> Document {
    let mut tok = Tokenizer::new(input);
    let mut tb = TreeBuilder::new();
    loop {
        let token = tok.next_token();
        let done = token == Token::Eof;
        tb.process(token);
        if done {
            break;
        }
    }
    tb.finish()
}

/// Parse an HTML fragment and append clones of the resulting body children to
/// `parent` in `doc`. Used by innerHTML.
pub fn set_inner_html(doc: &mut Document, parent: NodeId, html: &str) {
    let old_children = std::mem::take(&mut doc.nodes[parent].children);
    for c in old_children {
        doc.nodes[c].parent = None;
    }
    append_html(doc, parent, html);
}

/// Parse an HTML fragment and append its nodes to `parent` without clearing.
pub fn append_html(doc: &mut Document, parent: NodeId, html: &str) {
    let mut fragment = parse_document(html);
    let body = fragment
        .walk(fragment.root)
        .into_iter()
        .find(|&n| fragment.tag(n) == Some("body"));
    let children = match body {
        Some(b) => fragment.nodes[b].children.clone(),
        None => Vec::new(),
    };
    let mut new_nodes = Vec::new();
    for c in children {
        let cloned = fragment.deep_clone(c);
        new_nodes.push(cloned);
    }
    // Move nodes from the fragment document into `doc`.
    for n in new_nodes {
        let transferred = transfer_subtree(&mut fragment, doc, n);
        doc.remove_child(parent, transferred);
        doc.append_child(parent, transferred);
    }
}

fn transfer_subtree(from: &mut Document, to: &mut Document, node: NodeId) -> NodeId {
    let data = from.nodes[node].data.clone();
    let new_id = to.add_node(data);
    let children = from.nodes[node].children.clone();
    for c in children {
        let nc = transfer_subtree(from, to, c);
        to.append_child(new_id, nc);
    }
    new_id
}
