#![allow(dead_code)]
//! CSS parsing: stylesheets, selectors, media queries and value helpers.

use crate::dom::{Document, NodeId};
use crate::util::{named_color, parse_color, Color};

#[derive(Clone, Debug, Default)]
pub struct StyleSheet {
    pub rules: Vec<Rule>,
    pub order: usize,
}

#[derive(Clone, Debug)]
pub struct Rule {
    pub selectors: Vec<Selector>,
    pub decls: Vec<Declaration>,
    pub media: Option<String>,
    pub order: usize,
}

#[derive(Clone, Debug, PartialEq)]
pub struct Declaration {
    pub name: String,
    pub value: String,
    pub important: bool,
}

#[derive(Clone, Debug, PartialEq)]
pub struct Selector {
    pub compounds: Vec<Compound>,
    pub combinators: Vec<Combinator>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Combinator {
    Descendant,
    Child,
    Adjacent,
    Sibling,
}

#[derive(Clone, Debug, PartialEq, Default)]
pub struct Compound {
    pub simples: Vec<Simple>,
    pub pseudo_element: Option<String>,
}

#[derive(Clone, Debug, PartialEq)]
pub enum Simple {
    Type(String),
    Universal,
    Id(String),
    Class(String),
    Attr(String, Option<(AttrOp, String)>),
    Pseudo(PseudoClass),
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum AttrOp {
    Equals,
    Includes,
    Dash,
    Prefix,
    Suffix,
    Contains,
}

#[derive(Clone, Debug, PartialEq)]
pub enum PseudoClass {
    FirstChild,
    LastChild,
    OnlyChild,
    FirstOfType,
    LastOfType,
    OnlyOfType,
    NthChild(i32, i32),
    NthLastChild(i32, i32),
    NthOfType(i32, i32),
    Not(Vec<Compound>),
    Empty,
    Root,
    Checked,
    Disabled,
    Enabled,
    Link,
    Hover,
    Focus,
    Active,
    Visited,
    Target,
    Required,
    Optional,
    ReadOnly,
    ReadWrite,
    PlaceholderShown,
    FocusWithin,
    FocusVisible,
}

pub struct MatchContext<'a> {
    pub doc: &'a Document,
    pub hovered: Option<NodeId>,
    pub active: Option<NodeId>,
    pub focused: Option<NodeId>,
}

fn is_descendant_of(doc: &Document, node: NodeId, ancestor: NodeId) -> bool {
    let mut cur = Some(node);
    while let Some(n) = cur {
        if n == ancestor {
            return true;
        }
        cur = doc.nodes[n].parent;
    }
    false
}

impl Selector {
    pub fn specificity(&self) -> (u32, u32, u32) {
        let mut a = 0;
        let mut b = 0;
        let mut c = 0;
        for comp in &self.compounds {
            for s in &comp.simples {
                match s {
                    Simple::Id(_) => a += 1,
                    Simple::Class(_) | Simple::Attr(..) | Simple::Pseudo(_) => b += 1,
                    Simple::Type(_) => c += 1,
                    Simple::Universal => {}
                }
            }
            if comp.pseudo_element.is_some() {
                c += 1;
            }
        }
        (a, b, c)
    }

    pub fn matches(&self, node: NodeId, ctx: &MatchContext) -> bool {
        if self
            .compounds
            .last()
            .map(|c| c.pseudo_element.is_some())
            .unwrap_or(false)
        {
            return false;
        }
        self.match_at(node, self.compounds.len() - 1, ctx)
    }

    /// Match against an element for a generated-content pseudo element
    /// (`::before` / `::after`).
    pub fn matches_pseudo(&self, node: NodeId, ctx: &MatchContext, which: &str) -> bool {
        match self.compounds.last().and_then(|c| c.pseudo_element.as_deref()) {
            Some(p) if p == which => self.match_at(node, self.compounds.len() - 1, ctx),
            _ => false,
        }
    }

    fn match_at(&self, node: NodeId, idx: usize, ctx: &MatchContext) -> bool {
        if !compound_matches(&self.compounds[idx], node, ctx) {
            return false;
        }
        if idx == 0 {
            return true;
        }
        let comb = self.combinators[idx - 1];
        match comb {
            Combinator::Child => {
                if let Some(parent) = ctx.doc.nodes[node].parent {
                    if ctx.doc.is_element(parent) {
                        return self.match_at(parent, idx - 1, ctx);
                    }
                }
                false
            }
            Combinator::Descendant => {
                let mut cur = ctx.doc.nodes[node].parent;
                while let Some(n) = cur {
                    if ctx.doc.is_element(n) && self.match_at(n, idx - 1, ctx) {
                        return true;
                    }
                    cur = ctx.doc.nodes[n].parent;
                }
                false
            }
            Combinator::Adjacent => {
                let mut prev = ctx.doc.prev_element_sibling(node);
                if let Some(p) = prev.take() {
                    return self.match_at(p, idx - 1, ctx);
                }
                false
            }
            Combinator::Sibling => {
                let mut cur = ctx.doc.prev_element_sibling(node);
                while let Some(n) = cur {
                    if self.match_at(n, idx - 1, ctx) {
                        return true;
                    }
                    cur = ctx.doc.prev_element_sibling(n);
                }
                false
            }
        }
    }
}

fn compound_matches(comp: &Compound, node: NodeId, ctx: &MatchContext) -> bool {
    let doc = ctx.doc;
    let el = match doc.element(node) {
        Some(e) => e,
        None => return false,
    };
    for s in &comp.simples {
        let ok = match s {
            Simple::Universal => true,
            Simple::Type(t) => el.tag == *t,
            Simple::Id(id) => el.id() == Some(id.as_str()),
            Simple::Class(c) => el.class_list().iter().any(|x| *x == c.as_str()),
            Simple::Attr(name, test) => match el.attr(name) {
                None => false,
                Some(v) => match test {
                    None => true,
                    Some((op, expected)) => match op {
                        AttrOp::Equals => v == expected,
                        AttrOp::Includes => v.split_whitespace().any(|t| t == expected),
                        AttrOp::Dash => v == expected || v.starts_with(&format!("{}-", expected)),
                        AttrOp::Prefix => v.starts_with(expected.as_str()),
                        AttrOp::Suffix => v.ends_with(expected.as_str()),
                        AttrOp::Contains => v.contains(expected.as_str()),
                    },
                },
            },
            Simple::Pseudo(p) => pseudo_matches(p, node, ctx),
        };
        if !ok {
            return false;
        }
    }
    true
}

fn element_index(node: NodeId, doc: &Document, of_type: bool) -> (usize, usize) {
    let parent = match doc.nodes[node].parent {
        Some(p) => p,
        None => return (1, 1),
    };
    let tag = doc.tag(node).unwrap_or("");
    let mut idx = 0;
    let mut total = 0;
    let mut found = 1;
    for &c in &doc.nodes[parent].children {
        if !doc.is_element(c) {
            continue;
        }
        if of_type && doc.tag(c) != Some(tag) {
            continue;
        }
        idx += 1;
        total += 1;
        if c == node {
            found = idx;
        }
    }
    (found, total)
}

fn nth_matches(anb: (i32, i32), index: usize) -> bool {
    let (a, b) = anb;
    let i = index as i32;
    if a == 0 {
        return i == b;
    }
    let diff = i - b;
    diff % a == 0 && diff / a >= 0
}

fn parse_nth(s: &str) -> Option<(i32, i32)> {
    let s = s.trim().to_ascii_lowercase().replace(' ', "");
    if s == "odd" {
        return Some((2, 1));
    }
    if s == "even" {
        return Some((2, 0));
    }
    if let Ok(n) = s.parse::<i32>() {
        return Some((0, n));
    }
    if let Some(idx) = s.find('n') {
        let a_part = &s[..idx];
        let b_part = &s[idx + 1..];
        let a = if a_part.is_empty() || a_part == "+" {
            1
        } else if a_part == "-" {
            -1
        } else {
            a_part.parse::<i32>().ok()?
        };
        let b = if b_part.is_empty() {
            0
        } else {
            b_part.parse::<i32>().ok()?
        };
        return Some((a, b));
    }
    None
}

fn pseudo_matches(p: &PseudoClass, node: NodeId, ctx: &MatchContext) -> bool {
    let doc = ctx.doc;
    let el = match doc.element(node) {
        Some(e) => e,
        None => return false,
    };
    match p {
        PseudoClass::FirstChild => element_index(node, doc, false).0 == 1,
        PseudoClass::LastChild => {
            let (i, t) = element_index(node, doc, false);
            i == t
        }
        PseudoClass::OnlyChild => element_index(node, doc, false).1 == 1,
        PseudoClass::FirstOfType => element_index(node, doc, true).0 == 1,
        PseudoClass::LastOfType => {
            let (i, t) = element_index(node, doc, true);
            i == t
        }
        PseudoClass::OnlyOfType => element_index(node, doc, true).1 == 1,
        PseudoClass::NthChild(a, b) => nth_matches((*a, *b), element_index(node, doc, false).0),
        PseudoClass::NthLastChild(a, b) => {
            let (i, t) = element_index(node, doc, false);
            nth_matches((*a, *b), t - i + 1)
        }
        PseudoClass::NthOfType(a, b) => nth_matches((*a, *b), element_index(node, doc, true).0),
        PseudoClass::Not(comps) => !comps.iter().any(|c| compound_matches(c, node, ctx)),
        PseudoClass::Empty => doc.nodes[node].children.iter().all(|&c| {
            doc.element(c).is_none() && doc.text(c).map(|t| t.trim().is_empty()).unwrap_or(true)
        }),
        PseudoClass::Root => doc.nodes[node].parent.map(|p| p == doc.root).unwrap_or(false),
        PseudoClass::Checked => el.attr("checked").is_some() || el.attr("selected").is_some(),
        PseudoClass::Disabled => el.attr("disabled").is_some(),
        PseudoClass::Enabled => el.attr("disabled").is_none(),
        PseudoClass::Link => {
            matches!(el.tag.as_str(), "a" | "area") && el.attr("href").is_some()
        }
        PseudoClass::Hover => ctx
            .hovered
            .map(|h| is_descendant_of(doc, h, node))
            .unwrap_or(false),
        PseudoClass::Active => ctx
            .active
            .map(|h| is_descendant_of(doc, h, node))
            .unwrap_or(false),
        PseudoClass::Focus | PseudoClass::FocusVisible | PseudoClass::FocusWithin => ctx
            .focused
            .map(|f| f == node || (matches!(p, PseudoClass::FocusWithin) && is_descendant_of(doc, f, node)))
            .unwrap_or(false),
        PseudoClass::Visited => false,
        PseudoClass::Target => false,
        PseudoClass::Required => el.attr("required").is_some(),
        PseudoClass::Optional => el.attr("required").is_none(),
        PseudoClass::ReadOnly => el.attr("readonly").is_some(),
        PseudoClass::ReadWrite => el.attr("readonly").is_none(),
        PseudoClass::PlaceholderShown => el.attr("placeholder").is_some(),
    }
}

// ---------------------------------------------------------------- parsing

fn strip_comments(src: &str) -> String {
    let chars: Vec<char> = src.chars().collect();
    let mut out = String::with_capacity(src.len());
    let mut i = 0;
    while i < chars.len() {
        if i + 1 < chars.len() && chars[i] == '/' && chars[i + 1] == '*' {
            i += 2;
            while i + 1 < chars.len() && !(chars[i] == '*' && chars[i + 1] == '/') {
                i += 1;
            }
            i += 2;
        } else {
            out.push(chars[i]);
            i += 1;
        }
    }
    out
}

pub fn parse_stylesheet(src: &str) -> StyleSheet {
    let mut sheet = StyleSheet::default();
    let cleaned = strip_comments(src);
    let chars: Vec<char> = cleaned.chars().collect();
    let mut p = CssParser { chars, pos: 0 };
    p.parse_rules(&mut sheet, None, true);
    sheet
}

pub fn parse_declarations(src: &str) -> Vec<Declaration> {
    let chars: Vec<char> = src.chars().collect();
    let mut p = CssParser { chars, pos: 0 };
    p.parse_declaration_block(None)
}

struct CssParser {
    chars: Vec<char>,
    pos: usize,
}

impl CssParser {
    fn peek(&self) -> Option<char> {
        self.chars.get(self.pos).copied()
    }
    fn peek_at(&self, n: usize) -> Option<char> {
        self.chars.get(self.pos + n).copied()
    }
    fn skip_ws(&mut self) {
        while let Some(c) = self.peek() {
            if c.is_whitespace() {
                self.pos += 1;
            } else {
                break;
            }
        }
    }
    fn starts_with_ci(&self, s: &str) -> bool {
        let mut i = self.pos;
        for c in s.chars() {
            if i >= self.chars.len() || !self.chars[i].eq_ignore_ascii_case(&c) {
                return false;
            }
            i += 1;
        }
        true
    }
    fn scan_until(&mut self, stop: &[char]) -> String {
        let mut out = String::new();
        let mut quote: Option<char> = None;
        let mut depth = 0i32;
        while let Some(c) = self.peek() {
            if let Some(q) = quote {
                out.push(c);
                self.pos += 1;
                if c == q {
                    quote = None;
                }
                continue;
            }
            match c {
                '"' | '\'' => {
                    quote = Some(c);
                    out.push(c);
                    self.pos += 1;
                }
                '(' | '[' => {
                    depth += 1;
                    out.push(c);
                    self.pos += 1;
                }
                ')' | ']' => {
                    depth -= 1;
                    out.push(c);
                    self.pos += 1;
                }
                _ if depth <= 0 && stop.contains(&c) => break,
                _ => {
                    out.push(c);
                    self.pos += 1;
                }
            }
        }
        out
    }
    fn skip_block(&mut self) {
        // pos at '{'
        if self.peek() == Some('{') {
            self.pos += 1;
        }
        let mut depth = 1;
        let mut quote: Option<char> = None;
        while let Some(c) = self.peek() {
            self.pos += 1;
            if let Some(q) = quote {
                if c == q {
                    quote = None;
                }
                continue;
            }
            match c {
                '"' | '\'' => quote = Some(c),
                '{' => depth += 1,
                '}' => {
                    depth -= 1;
                    if depth == 0 {
                        break;
                    }
                }
                _ => {}
            }
        }
    }

    fn parse_rules(&mut self, sheet: &mut StyleSheet, media: Option<String>, top: bool) {
        loop {
            self.skip_ws();
            let c = match self.peek() {
                Some(c) => c,
                None => break,
            };
            if c == '}' {
                self.pos += 1;
                break;
            }
            if c == '@' {
                self.parse_at_rule(sheet, media.clone(), top);
                continue;
            }
            // selector text until '{'
            let sel_text = self.scan_until(&['{', '}', ';']);
            match self.peek() {
                Some('{') => {
                    self.pos += 1;
                    let decls = self.parse_declaration_block(media.clone());
                    let mut selectors = Vec::new();
                    for part in split_top_level(&sel_text, ',') {
                        if let Some(sel) = parse_selector(&part) {
                            selectors.push(sel);
                        }
                    }
                    if !selectors.is_empty() {
                        sheet.rules.push(Rule {
                            selectors,
                            decls,
                            media: media.clone(),
                            order: sheet.order,
                        });
                        sheet.order += 1;
                    }
                }
                Some(';') => {
                    self.pos += 1;
                }
                Some('}') => {
                    self.pos += 1;
                    break;
                }
                _ => break,
            }
        }
    }

    fn parse_at_rule(&mut self, sheet: &mut StyleSheet, media: Option<String>, _top: bool) {
        self.pos += 1;
        let name = self.read_ident().to_ascii_lowercase();
        match name.as_str() {
            "media" => {
                let cond = self.scan_until(&['{']);
                if self.peek() == Some('{') {
                    self.pos += 1;
                    let combined = match &media {
                        Some(m) if !m.is_empty() => Some(format!("{} and {}", m, cond.trim())),
                        _ => Some(cond.trim().to_string()),
                    };
                    self.parse_rules(sheet, combined, false);
                }
            }
            "supports" | "layer" | "container" => {
                self.scan_until(&['{']);
                if self.peek() == Some('{') {
                    self.pos += 1;
                    self.parse_rules(sheet, media, false);
                }
            }
            "import" | "charset" | "namespace" => {
                self.scan_until(&[';', '{', '}']);
                if self.peek() == Some(';') {
                    self.pos += 1;
                }
            }
            "keyframes" | "-webkit-keyframes" | "font-face" | "page" | "property" | "counter-style" => {
                // Skip prelude and block.
                self.scan_until(&['{', ';']);
                if self.peek() == Some('{') {
                    self.skip_block();
                } else if self.peek() == Some(';') {
                    self.pos += 1;
                }
            }
            _ => {
                self.scan_until(&['{', ';']);
                if self.peek() == Some('{') {
                    self.skip_block();
                } else if self.peek() == Some(';') {
                    self.pos += 1;
                }
            }
        }
    }

    fn parse_declaration_block(&mut self, _media: Option<String>) -> Vec<Declaration> {
        let mut out = Vec::new();
        loop {
            self.skip_ws();
            match self.peek() {
                None => break,
                Some('}') => {
                    self.pos += 1;
                    break;
                }
                _ => {}
            }
            let text = self.scan_until(&[';', '}']);
            let text = text.trim();
            if !text.is_empty() {
                if let Some(d) = parse_declaration(text) {
                    out.push(d);
                }
            }
            if self.peek() == Some(';') {
                self.pos += 1;
            } else if self.peek() == Some('}') {
                self.pos += 1;
                break;
            }
        }
        out
    }

    fn read_ident(&mut self) -> String {
        let mut s = String::new();
        while let Some(c) = self.peek() {
            if c.is_alphanumeric() || c == '-' || c == '_' || c == '\\' {
                s.push(c);
                self.pos += 1;
            } else {
                break;
            }
        }
        s
    }
}

fn parse_declaration(text: &str) -> Option<Declaration> {
    let idx = split_top_level_index(text, ':')?;
    let raw_name = text[..idx].trim();
    let mut name = if raw_name.starts_with("--") {
        raw_name.to_string()
    } else {
        raw_name.to_ascii_lowercase()
    };
    let mut value = text[idx + 1..].trim().to_string();
    if name.is_empty() || value.is_empty() {
        return None;
    }
    let mut important = false;
    let low = value.to_ascii_lowercase();
    if let Some(stripped) = low.strip_suffix("!important").or_else(|| low.strip_suffix("! important")) {
        let cut = stripped.len();
        value = value[..cut].trim_end().to_string();
        important = true;
    }
    if value.is_empty() {
        return None;
    }
    Some(Declaration { name, value, important })
}

pub fn split_top_level(s: &str, sep: char) -> Vec<String> {
    let mut out = Vec::new();
    let mut cur = String::new();
    let mut depth = 0i32;
    let mut quote: Option<char> = None;
    for c in s.chars() {
        if let Some(q) = quote {
            cur.push(c);
            if c == q {
                quote = None;
            }
            continue;
        }
        match c {
            '"' | '\'' => {
                quote = Some(c);
                cur.push(c);
            }
            '(' | '[' => {
                depth += 1;
                cur.push(c);
            }
            ')' | ']' => {
                depth -= 1;
                cur.push(c);
            }
            _ if depth <= 0 && c == sep => {
                out.push(cur.trim().to_string());
                cur.clear();
            }
            _ => cur.push(c),
        }
    }
    if !cur.trim().is_empty() {
        out.push(cur.trim().to_string());
    }
    out
}

fn split_top_level_index(s: &str, sep: char) -> Option<usize> {
    let mut depth = 0i32;
    let mut quote: Option<char> = None;
    for (i, c) in s.char_indices() {
        if let Some(q) = quote {
            if c == q {
                quote = None;
            }
            continue;
        }
        match c {
            '"' | '\'' => quote = Some(c),
            '(' | '[' => depth += 1,
            ')' | ']' => depth -= 1,
            _ if depth <= 0 && c == sep => return Some(i),
            _ => {}
        }
    }
    None
}

pub fn parse_selector(text: &str) -> Option<Selector> {
    let chars: Vec<char> = text.trim().chars().collect();
    let mut i = 0;
    let mut compounds = Vec::new();
    let mut combinators = Vec::new();

    fn skip_ws(chars: &[char], i: &mut usize) -> bool {
        let start = *i;
        while *i < chars.len() && chars[*i].is_whitespace() {
            *i += 1;
        }
        *i > start
    }

    skip_ws(&chars, &mut i);
    if i >= chars.len() {
        return None;
    }
    loop {
        let (comp, ni) = parse_compound(&chars, i)?;
        compounds.push(comp);
        i = ni;
        let had_ws = skip_ws(&chars, &mut i);
        if i >= chars.len() {
            break;
        }
        let comb = match chars[i] {
            '>' => {
                i += 1;
                Combinator::Child
            }
            '+' => {
                i += 1;
                Combinator::Adjacent
            }
            '~' => {
                i += 1;
                Combinator::Sibling
            }
            _ if had_ws => Combinator::Descendant,
            _ => return None,
        };
        combinators.push(comb);
        skip_ws(&chars, &mut i);
        if i >= chars.len() {
            return None;
        }
    }
    if compounds.len() != combinators.len() + 1 {
        return None;
    }
    Some(Selector { compounds, combinators })
}

fn read_name(chars: &[char], i: &mut usize) -> String {
    let mut s = String::new();
    while *i < chars.len() {
        let c = chars[*i];
        if c.is_alphanumeric() || c == '-' || c == '_' {
            s.push(c);
            *i += 1;
        } else if c == '\\' && *i + 1 < chars.len() {
            s.push(chars[*i + 1]);
            *i += 2;
        } else {
            break;
        }
    }
    s
}

fn parse_compound(chars: &[char], mut i: usize) -> Option<(Compound, usize)> {
    let mut comp = Compound::default();
    let mut any = false;
    loop {
        if i >= chars.len() {
            break;
        }
        match chars[i] {
            '*' => {
                comp.simples.push(Simple::Universal);
                i += 1;
                any = true;
            }
            '#' => {
                i += 1;
                let name = read_name(chars, &mut i);
                if name.is_empty() {
                    return None;
                }
                comp.simples.push(Simple::Id(name));
                any = true;
            }
            '.' => {
                i += 1;
                let name = read_name(chars, &mut i);
                if name.is_empty() {
                    return None;
                }
                comp.simples.push(Simple::Class(name));
                any = true;
            }
            '[' => {
                i += 1;
                while i < chars.len() && chars[i].is_whitespace() {
                    i += 1;
                }
                let name = read_name(chars, &mut i).to_ascii_lowercase();
                while i < chars.len() && chars[i].is_whitespace() {
                    i += 1;
                }
                let mut test = None;
                if i < chars.len() && chars[i] != ']' {
                    let op = match chars[i] {
                        '=' => {
                            i += 1;
                            AttrOp::Equals
                        }
                        '~' => {
                            i += 2;
                            AttrOp::Includes
                        }
                        '|' => {
                            i += 2;
                            AttrOp::Dash
                        }
                        '^' => {
                            i += 2;
                            AttrOp::Prefix
                        }
                        '$' => {
                            i += 2;
                            AttrOp::Suffix
                        }
                        '*' => {
                            i += 2;
                            AttrOp::Contains
                        }
                        _ => return None,
                    };
                    while i < chars.len() && chars[i].is_whitespace() {
                        i += 1;
                    }
                    let value = if i < chars.len() && (chars[i] == '"' || chars[i] == '\'') {
                        let q = chars[i];
                        i += 1;
                        let start = i;
                        while i < chars.len() && chars[i] != q {
                            i += 1;
                        }
                        let v: String = chars[start..i].iter().collect();
                        i += 1;
                        v
                    } else {
                        read_name(chars, &mut i)
                    };
                    while i < chars.len() && chars[i] != ']' {
                        i += 1;
                    }
                    test = Some((op, value));
                }
                if i < chars.len() && chars[i] == ']' {
                    i += 1;
                }
                comp.simples.push(Simple::Attr(name, test));
                any = true;
            }
            ':' => {
                let is_element = chars.get(i + 1) == Some(&':');
                i += if is_element { 2 } else { 1 };
                let name = read_name(chars, &mut i).to_ascii_lowercase();
                if is_element {
                    comp.pseudo_element = Some(name);
                    any = true;
                    continue;
                }
                let mut args = String::new();
                if i < chars.len() && chars[i] == '(' {
                    i += 1;
                    let start = i;
                    let mut depth = 1;
                    while i < chars.len() {
                        if chars[i] == '(' {
                            depth += 1;
                        } else if chars[i] == ')' {
                            depth -= 1;
                            if depth == 0 {
                                break;
                            }
                        }
                        i += 1;
                    }
                    args = chars[start..i].iter().collect();
                    if i < chars.len() {
                        i += 1;
                    }
                }
                let pc = match name.as_str() {
                    "first-child" => PseudoClass::FirstChild,
                    "last-child" => PseudoClass::LastChild,
                    "only-child" => PseudoClass::OnlyChild,
                    "first-of-type" => PseudoClass::FirstOfType,
                    "last-of-type" => PseudoClass::LastOfType,
                    "only-of-type" => PseudoClass::OnlyOfType,
                    "nth-child" => {
                        let (a, b) = parse_nth(&args)?;
                        PseudoClass::NthChild(a, b)
                    }
                    "nth-last-child" => {
                        let (a, b) = parse_nth(&args)?;
                        PseudoClass::NthLastChild(a, b)
                    }
                    "nth-of-type" => {
                        let (a, b) = parse_nth(&args)?;
                        PseudoClass::NthOfType(a, b)
                    }
                    "not" => {
                        let mut comps = Vec::new();
                        for part in split_top_level(&args, ',') {
                            if let Some(sel) = parse_selector(&part) {
                                if sel.compounds.len() == 1 {
                                    comps.push(sel.compounds[0].clone());
                                }
                            }
                        }
                        PseudoClass::Not(comps)
                    }
                    "empty" => PseudoClass::Empty,
                    "root" => PseudoClass::Root,
                    "checked" => PseudoClass::Checked,
                    "disabled" => PseudoClass::Disabled,
                    "enabled" => PseudoClass::Enabled,
                    "link" | "any-link" => PseudoClass::Link,
                    "hover" => PseudoClass::Hover,
                    "focus" | "focus-visible" => PseudoClass::Focus,
                    "focus-within" => PseudoClass::FocusWithin,
                    "active" => PseudoClass::Active,
                    "visited" => PseudoClass::Visited,
                    "target" => PseudoClass::Target,
                    "required" => PseudoClass::Required,
                    "optional" => PseudoClass::Optional,
                    "read-only" => PseudoClass::ReadOnly,
                    "read-write" => PseudoClass::ReadWrite,
                    "placeholder-shown" => PseudoClass::PlaceholderShown,
                    // Unknown pseudo classes: treat as never matching.
                    _ => return None,
                };
                comp.simples.push(Simple::Pseudo(pc));
                any = true;
            }
            c if c.is_ascii_alphabetic() || c == '_' => {
                let name = read_name(chars, &mut i).to_ascii_lowercase();
                comp.simples.push(Simple::Type(name));
                any = true;
            }
            _ => break,
        }
    }
    if !any {
        return None;
    }
    Some((comp, i))
}

// ---------------------------------------------------------------- values

#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Length {
    Auto,
    Px(f32),
    Percent(f32),
}

impl Length {
    pub fn is_auto(&self) -> bool {
        matches!(self, Length::Auto)
    }
}

pub fn parse_length(value: &str, font_size: f32, root_font_size: f32, viewport: (f32, f32)) -> Option<Length> {
    let v = value.trim().to_ascii_lowercase();
    if v == "auto" {
        return Some(Length::Auto);
    }
    if v == "0" || v == "0px" {
        return Some(Length::Px(0.0));
    }
    if let Some(inner) = v.strip_prefix("calc(") {
        if let Some(inner) = inner.strip_suffix(')') {
            return eval_calc(inner, font_size, root_font_size, viewport);
        }
    }
    parse_length_units(&v, font_size, root_font_size, viewport)
}

fn parse_length_units(v: &str, font_size: f32, root_font_size: f32, viewport: (f32, f32)) -> Option<Length> {
    let split = v
        .char_indices()
        .find(|(_, c)| c.is_ascii_alphabetic() || *c == '%')
        .map(|(i, _)| i)
        .unwrap_or(v.len());
    let (num_str, unit) = v.split_at(split);
    let num: f32 = num_str.trim().parse().ok()?;
    let px = match unit.trim() {
        "px" => num,
        "pt" => num * 96.0 / 72.0,
        "pc" => num * 16.0,
        "in" => num * 96.0,
        "cm" => num * 96.0 / 2.54,
        "mm" => num * 96.0 / 25.4,
        "q" => num * 96.0 / 101.6,
        "em" => num * font_size,
        "rem" => num * root_font_size,
        "ex" | "ch" => num * font_size * 0.5,
        "vw" => num * viewport.0 / 100.0,
        "vh" => num * viewport.1 / 100.0,
        "vmin" => num * viewport.0.min(viewport.1) / 100.0,
        "vmax" => num * viewport.0.max(viewport.1) / 100.0,
        "%" => return Some(Length::Percent(num / 100.0)),
        "" => {
            if num == 0.0 {
                0.0
            } else {
                return None;
            }
        }
        _ => return None,
    };
    Some(Length::Px(px))
}

fn eval_calc(expr: &str, fs: f32, rfs: f32, vp: (f32, f32)) -> Option<Length> {
    // Very small calc evaluator: supports +, -, *, / with length/number terms.
    #[derive(Clone, Copy)]
    enum V {
        Len(Length),
        Num(f32),
    }
    fn term(s: &str, fs: f32, rfs: f32, vp: (f32, f32)) -> Option<V> {
        let s = s.trim();
        if let Some(inner) = s.strip_prefix('(').and_then(|x| x.strip_suffix(')')) {
            return eval_v(inner, fs, rfs, vp);
        }
        if let Some(l) = parse_length_units(&s.to_ascii_lowercase(), fs, rfs, vp) {
            return Some(V::Len(l));
        }
        s.parse::<f32>().ok().map(V::Num)
    }
    fn eval_v(expr: &str, fs: f32, rfs: f32, vp: (f32, f32)) -> Option<V> {
        // tokenize respecting parentheses
        let mut parts: Vec<String> = Vec::new();
        let mut cur = String::new();
        let mut depth = 0;
        for c in expr.chars() {
            match c {
                '(' => {
                    depth += 1;
                    cur.push(c);
                }
                ')' => {
                    depth -= 1;
                    cur.push(c);
                }
                '+' | '-' | '*' | '/' if depth == 0 => {
                    if !cur.trim().is_empty() {
                        parts.push(cur.trim().to_string());
                    }
                    cur.clear();
                    parts.push(c.to_string());
                }
                _ => cur.push(c),
            }
        }
        if !cur.trim().is_empty() {
            parts.push(cur.trim().to_string());
        }
        if parts.is_empty() {
            return None;
        }
        // Handle leading unary minus.
        let mut idx = 0;
        let mut acc = if parts[0] == "-" {
            idx = 1;
            let t = term(&parts[1], fs, rfs, vp)?;
            idx = 2;
            match t {
                V::Len(Length::Px(v)) => V::Len(Length::Px(-v)),
                V::Len(Length::Percent(v)) => V::Len(Length::Percent(-v)),
                V::Len(_) => return None,
                V::Num(n) => V::Num(-n),
            }
        } else {
            idx = 1;
            term(&parts[0], fs, rfs, vp)?
        };
        while idx + 1 < parts.len() {
            let op = parts[idx].clone();
            let rhs = term(&parts[idx + 1], fs, rfs, vp)?;
            acc = match (op.as_str(), acc, rhs) {
                ("+", V::Len(a), V::Len(b)) => V::Len(add_len(a, b)?),
                ("-", V::Len(a), V::Len(b)) => V::Len(add_len(a, neg_len(b))?),
                ("*", V::Len(a), V::Num(n)) | ("*", V::Num(n), V::Len(a)) => V::Len(mul_len(a, n)),
                ("/", V::Len(a), V::Num(n)) if n != 0.0 => V::Len(mul_len(a, 1.0 / n)),
                ("+", V::Num(a), V::Num(b)) => V::Num(a + b),
                ("-", V::Num(a), V::Num(b)) => V::Num(a - b),
                ("*", V::Num(a), V::Num(b)) => V::Num(a * b),
                ("/", V::Num(a), V::Num(b)) if b != 0.0 => V::Num(a / b),
                _ => return None,
            };
            idx += 2;
        }
        Some(acc)
    }
    fn neg_len(l: Length) -> Length {
        match l {
            Length::Px(v) => Length::Px(-v),
            Length::Percent(p) => Length::Percent(-p),
            Length::Auto => Length::Auto,
        }
    }
    fn add_len(a: Length, b: Length) -> Option<Length> {
        match (a, b) {
            (Length::Px(x), Length::Px(y)) => Some(Length::Px(x + y)),
            (Length::Percent(x), Length::Percent(y)) => Some(Length::Percent(x + y)),
            _ => None,
        }
    }
    fn mul_len(a: Length, n: f32) -> Length {
        match a {
            Length::Px(v) => Length::Px(v * n),
            Length::Percent(v) => Length::Percent(v * n),
            Length::Auto => Length::Auto,
        }
    }
    match eval_v(expr, fs, rfs, vp)? {
        V::Len(l) => Some(l),
        V::Num(n) => Some(Length::Px(n)),
    }
}

pub fn resolve_length(len: Length, basis: f32) -> Option<f32> {
    match len {
        Length::Px(v) => Some(v),
        Length::Percent(p) => Some(basis * p),
        Length::Auto => None,
    }
}

pub fn parse_font_size(value: &str, parent: f32, root: f32, viewport: (f32, f32)) -> Option<f32> {
    let v = value.trim().to_ascii_lowercase();
    let keyword = match v.as_str() {
        "xx-small" => Some(9.0),
        "x-small" => Some(10.0),
        "small" => Some(13.0),
        "medium" => Some(16.0),
        "large" => Some(18.0),
        "x-large" => Some(24.0),
        "xx-large" => Some(32.0),
        "xxx-large" => Some(48.0),
        "smaller" => Some(parent * 0.83),
        "larger" => Some(parent * 1.2),
        _ => None,
    };
    if let Some(k) = keyword {
        return Some(k);
    }
    parse_length(&v, parent, root, viewport).and_then(|l| match l {
        Length::Px(px) => Some(px),
        Length::Percent(p) => Some(parent * p),
        Length::Auto => None,
    })
}

pub fn parse_color_value(value: &str, current: Color) -> Option<Color> {
    let v = value.trim();
    if v.eq_ignore_ascii_case("currentcolor") {
        return Some(current);
    }
    if v.eq_ignore_ascii_case("inherit") || v.eq_ignore_ascii_case("unset") || v.eq_ignore_ascii_case("initial") {
        return None;
    }
    if let Some(c) = parse_color(v) {
        return Some(c);
    }
    // System colors used by old pages.
    let low = v.to_ascii_lowercase();
    match low.as_str() {
        "canvas" | "window" | "background" | "buttonface" => Some(Color::WHITE),
        "canvastext" | "windowtext" | "buttontext" | "captiontext" => Some(Color::BLACK),
        "highlight" => Some(Color::rgb(0, 120, 215)),
        "highlighttext" => Some(Color::WHITE),
        "linktext" => Some(Color::rgb(0, 0, 238)),
        "visitedtext" => Some(Color::rgb(85, 26, 139)),
        "activeborder" | "inactiveborder" => Some(Color::rgb(200, 200, 200)),
        "graytext" => Some(Color::rgb(109, 109, 109)),
        _ => named_color(&low),
    }
}

/// Extract the first url(...) in a value.
pub fn extract_url(value: &str) -> Option<String> {
    let lower = value.to_ascii_lowercase();
    let idx = lower.find("url(")?;
    let rest = &value[idx + 4..];
    let rest = rest.trim_start();
    if let Some(inner) = rest.strip_prefix('"') {
        let end = inner.find('"')?;
        return Some(inner[..end].to_string());
    }
    if let Some(inner) = rest.strip_prefix('\'') {
        let end = inner.find('\'')?;
        return Some(inner[..end].to_string());
    }
    let end = rest.find(')')?;
    Some(rest[..end].trim().to_string())
}

pub fn media_matches(query: &str, viewport: (f32, f32)) -> bool {
    let q = query.trim().to_ascii_lowercase();
    if q.is_empty() {
        return true;
    }
    for part in split_top_level(&q, ',') {
        if media_query_one_matches(&part, viewport) {
            return true;
        }
    }
    false
}

fn media_query_one_matches(q: &str, viewport: (f32, f32)) -> bool {
    let q = q.trim();
    if q.is_empty() {
        return true;
    }
    let (negate, rest) = match q.strip_prefix("not ") {
        Some(r) => (true, r.trim()),
        None => (false, q),
    };
    let rest = rest.strip_prefix("only ").unwrap_or(rest).trim();
    let result = media_query_eval(rest, viewport);
    if negate {
        !result
    } else {
        result
    }
}

fn media_query_eval(q: &str, viewport: (f32, f32)) -> bool {
    // Media type is the leading bare word before any '(' or 'and'.
    let mut prelude = String::new();
    let mut features = String::new();
    let mut seen_paren = false;
    for c in q.chars() {
        if c == '(' {
            seen_paren = true;
        }
        if seen_paren {
            features.push(c);
        } else {
            prelude.push(c);
        }
    }
    let ty = prelude
        .split_whitespace()
        .next()
        .unwrap_or("")
        .to_ascii_lowercase();
    if !ty.is_empty()
        && ty != "all"
        && ty != "screen"
        && ty != "and"
        && ty != "only"
    {
        return false;
    }
    let mut i = 0;
    let chars: Vec<char> = features.chars().collect();
    while i < chars.len() {
        if chars[i] == '(' {
            let mut j = i + 1;
            let mut depth = 1;
            while j < chars.len() {
                if chars[j] == '(' {
                    depth += 1;
                } else if chars[j] == ')' {
                    depth -= 1;
                    if depth == 0 {
                        break;
                    }
                }
                j += 1;
            }
            let expr: String = chars[i + 1..j.min(chars.len())].iter().collect();
            if !media_feature_matches(&expr, viewport) {
                return false;
            }
            i = j + 1;
        } else {
            i += 1;
        }
    }
    true
}

/// Whether a stylesheet with the given media attribute should be loaded for
/// a screen browser at this viewport.
pub fn stylesheet_media_ok(media: &str, viewport: (f32, f32)) -> bool {
    let m = media.trim();
    if m.is_empty() {
        return true;
    }
    for part in split_top_level(m, ',') {
        if media_query_one_matches(&part, viewport) {
            return true;
        }
    }
    false
}

fn media_feature_matches(expr: &str, viewport: (f32, f32)) -> bool {
    let expr = expr.trim();
    if let Some(result) = media_range_matches(expr, viewport) {
        return result;
    }
    let (name, val) = match expr.split_once(':') {
        Some((n, v)) => (n.trim(), Some(v.trim())),
        None => (expr, None),
    };
    let parse_px = |s: &str| -> Option<f32> {
        parse_length(s, 16.0, 16.0, viewport).and_then(|l| resolve_length(l, viewport.0))
    };
    match name {
        "min-width" => val.and_then(parse_px).map(|v| viewport.0 >= v).unwrap_or(false),
        "max-width" => val.and_then(parse_px).map(|v| viewport.0 <= v).unwrap_or(false),
        "min-height" => val.and_then(parse_px).map(|v| viewport.1 >= v).unwrap_or(false),
        "max-height" => val.and_then(parse_px).map(|v| viewport.1 <= v).unwrap_or(false),
        "width" => val.and_then(parse_px).map(|v| (viewport.0 - v).abs() < 0.5).unwrap_or(false),
        "height" => val.and_then(parse_px).map(|v| (viewport.1 - v).abs() < 0.5).unwrap_or(false),
        "orientation" => {
            let landscape = viewport.0 >= viewport.1;
            match val {
                Some(v) if v.contains("landscape") => landscape,
                Some(v) if v.contains("portrait") => !landscape,
                _ => true,
            }
        }
        "prefers-color-scheme" => match val {
            Some(v) => v.contains("light") || v.contains("no-preference"),
            None => true,
        },
        "prefers-reduced-motion" => match val {
            Some(v) => v.contains("no-preference"),
            None => true,
        },
        "min-resolution" | "max-resolution" => true,
        "hover" | "pointer" | "any-hover" | "any-pointer" => true,
        "display-mode" => true,
        "color" => true,
        "monochrome" => val.map(|v| v == "0").unwrap_or(false),
        "scripting" => true,
        "scan" => true,
        "grid" => val.map(|v| v == "0").unwrap_or(true),
        _ => false,
    }
}

/// CSS Media Queries level 4 range syntax: `(width >= 600px)`,
/// `(400px < width <= 900px)`.
fn media_range_matches(expr: &str, viewport: (f32, f32)) -> Option<bool> {
    let parse_px = |s: &str| -> Option<f32> {
        let s = s.trim();
        if s.is_empty() {
            return None;
        }
        parse_length(s, 16.0, 16.0, viewport).and_then(|l| resolve_length(l, viewport.0))
    };
    let dim = |s: &str| -> Option<f32> {
        match s.trim().to_ascii_lowercase().as_str() {
            "width" | "inline-size" => Some(viewport.0),
            "height" | "block-size" => Some(viewport.1),
            _ => None,
        }
    };
    let cmp = |a: f32, op: &str, b: f32| -> bool {
        match op {
            "<" => a < b,
            "<=" => a <= b,
            ">" => a > b,
            ">=" => a >= b,
            "=" => (a - b).abs() < 0.5,
            _ => false,
        }
    };
    let find_op = |s: &str| -> Option<(usize, usize, &'static str)> {
        let bytes = s.as_bytes();
        let mut i = 0;
        while i < bytes.len() {
            match bytes[i] {
                b'<' | b'>' => {
                    let two = i + 1 < bytes.len() && bytes[i + 1] == b'=';
                    let op = match (bytes[i], two) {
                        (b'<', true) => "<=",
                        (b'>', true) => ">=",
                        (b'<', false) => "<",
                        _ => ">",
                    };
                    return Some((i, if two { 2 } else { 1 }, op));
                }
                _ => i += 1,
            }
        }
        None
    };
    let (i, oplen, op) = find_op(expr)?;
    let left = expr[..i].trim();
    let rest = expr[i + oplen..].trim();
    if let Some((j, oplen2, op2)) = find_op(rest) {
        let mid = rest[..j].trim();
        let right = rest[j + oplen2..].trim();
        let midv = dim(mid)?;
        let a = parse_px(left)?;
        let b = parse_px(right)?;
        return Some(cmp(midv, op, a) && cmp(midv, op2, b));
    }
    if let Some(v) = dim(left) {
        let b = parse_px(rest)?;
        Some(cmp(v, op, b))
    } else if let Some(v) = dim(rest) {
        let a = parse_px(left)?;
        let flipped = match op {
            "<" => ">",
            "<=" => ">=",
            ">" => "<",
            ">=" => "<=",
            other => other,
        };
        Some(cmp(v, flipped, a))
    } else {
        Some(false)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_simple_rules() {
        let s = parse_stylesheet("body { margin: 8px; color: red; }\nh1, p { font-size: 2em; }");
        println!("rules={}", s.rules.len());
        for r in &s.rules {
            println!("sel compounds={} decls={}", r.selectors.len(), r.decls.len());
            for d in &r.decls { println!("  {}: {}", d.name, d.value); }
        }
        assert_eq!(s.rules.len(), 2);
        assert_eq!(s.rules[0].decls.len(), 2);
        assert_eq!(s.rules[1].selectors.len(), 2);
    }

    #[test]
    fn match_body_selector() {
        let doc = crate::html::parse_document("<html><body><p>Hi</p></body></html>");
        let body = doc.walk(doc.root).into_iter().find(|&n| doc.tag(n) == Some("body")).unwrap();
        let s = parse_stylesheet("body { margin: 8px; }");
        let sel = &s.rules[0].selectors[0];
        let ctx = crate::css::MatchContext { doc: &doc, hovered: None, active: None, focused: None };
        println!("matches={}", sel.matches(body, &ctx));
        assert!(sel.matches(body, &ctx));
    }
}
