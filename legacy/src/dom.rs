#![allow(dead_code)]
use crate::util::escape_html;

pub type NodeId = usize;

#[derive(Debug, Clone)]
pub struct ElementData {
    pub tag: String,
    pub attrs: Vec<(String, String)>,
}

impl ElementData {
    pub fn attr(&self, name: &str) -> Option<&str> {
        self.attrs
            .iter()
            .find(|(n, _)| n.eq_ignore_ascii_case(name))
            .map(|(_, v)| v.as_str())
    }
    pub fn id(&self) -> Option<&str> {
        self.attr("id")
    }
    pub fn class_list(&self) -> Vec<&str> {
        self.attr("class")
            .map(|c| c.split_whitespace().collect())
            .unwrap_or_default()
    }
}

#[derive(Debug, Clone)]
pub enum NodeData {
    Document,
    Doctype(String),
    Element(ElementData),
    Text(String),
    Comment(String),
}

#[derive(Debug)]
pub struct Node {
    pub parent: Option<NodeId>,
    pub children: Vec<NodeId>,
    pub data: NodeData,
}

#[derive(Debug, Default)]
pub struct Document {
    pub nodes: Vec<Node>,
    pub root: NodeId,
}

impl Document {
    pub fn new() -> Document {
        let mut d = Document { nodes: Vec::new(), root: 0 };
        d.nodes.push(Node { parent: None, children: Vec::new(), data: NodeData::Document });
        d
    }

    pub fn add_node(&mut self, data: NodeData) -> NodeId {
        let id = self.nodes.len();
        self.nodes.push(Node { parent: None, children: Vec::new(), data });
        id
    }

    pub fn create_element(&mut self, tag: &str) -> NodeId {
        self.add_node(NodeData::Element(ElementData {
            tag: tag.to_ascii_lowercase(),
            attrs: Vec::new(),
        }))
    }

    pub fn create_text(&mut self, text: &str) -> NodeId {
        self.add_node(NodeData::Text(text.to_string()))
    }

    pub fn append_child(&mut self, parent: NodeId, child: NodeId) {
        self.detach(child);
        self.nodes[child].parent = Some(parent);
        self.nodes[parent].children.push(child);
    }

    pub fn insert_before(&mut self, parent: NodeId, child: NodeId, reference: Option<NodeId>) {
        self.detach(child);
        self.nodes[child].parent = Some(parent);
        match reference.and_then(|r| self.nodes[parent].children.iter().position(|&c| c == r)) {
            Some(idx) => self.nodes[parent].children.insert(idx, child),
            None => self.nodes[parent].children.push(child),
        }
    }

    pub fn detach(&mut self, node: NodeId) {
        if let Some(parent) = self.nodes[node].parent {
            if let Some(idx) = self.nodes[parent].children.iter().position(|&c| c == node) {
                self.nodes[parent].children.remove(idx);
            }
            self.nodes[node].parent = None;
        }
    }

    pub fn remove_child(&mut self, parent: NodeId, child: NodeId) {
        if self.nodes[child].parent == Some(parent) {
            self.nodes[parent].children.retain(|&c| c != child);
            self.nodes[child].parent = None;
        }
    }

    pub fn replace_all_children(&mut self, parent: NodeId, new_children: Vec<NodeId>) {
        let old = std::mem::take(&mut self.nodes[parent].children);
        for c in old {
            self.nodes[c].parent = None;
        }
        for c in new_children {
            self.detach(c);
            self.nodes[c].parent = Some(parent);
            self.nodes[parent].children.push(c);
        }
    }

    pub fn attr(&self, node: NodeId, name: &str) -> Option<&str> {
        self.element(node).and_then(|e| e.attr(name))
    }

    pub fn set_attr(&mut self, node: NodeId, name: &str, value: &str) {
        if let Some(el) = self.element_mut(node) {
            if let Some(slot) = el.attrs.iter_mut().find(|(n, _)| n.eq_ignore_ascii_case(name)) {
                slot.1 = value.to_string();
            } else {
                el.attrs.push((name.to_ascii_lowercase(), value.to_string()));
            }
        }
    }

    pub fn remove_attr(&mut self, node: NodeId, name: &str) {
        if let Some(el) = self.element_mut(node) {
            el.attrs.retain(|(n, _)| !n.eq_ignore_ascii_case(name));
        }
    }

    pub fn element(&self, node: NodeId) -> Option<&ElementData> {
        match &self.nodes[node].data {
            NodeData::Element(e) => Some(e),
            _ => None,
        }
    }

    pub fn element_mut(&mut self, node: NodeId) -> Option<&mut ElementData> {
        match &mut self.nodes[node].data {
            NodeData::Element(e) => Some(e),
            _ => None,
        }
    }

    pub fn tag(&self, node: NodeId) -> Option<&str> {
        self.element(node).map(|e| e.tag.as_str())
    }

    pub fn is_element(&self, node: NodeId) -> bool {
        matches!(self.nodes[node].data, NodeData::Element(_))
    }

    pub fn is_text(&self, node: NodeId) -> bool {
        matches!(self.nodes[node].data, NodeData::Text(_))
    }

    pub fn text(&self, node: NodeId) -> Option<&str> {
        match &self.nodes[node].data {
            NodeData::Text(t) => Some(t),
            _ => None,
        }
    }

    pub fn text_content(&self, node: NodeId) -> String {
        let mut out = String::new();
        self.collect_text(node, &mut out);
        out
    }

    fn collect_text(&self, node: NodeId, out: &mut String) {
        match &self.nodes[node].data {
            NodeData::Text(t) => out.push_str(t),
            _ => {
                for &c in &self.nodes[node].children {
                    self.collect_text(c, out);
                }
            }
        }
    }

    pub fn set_text_content(&mut self, node: NodeId, text: &str) {
        let old = std::mem::take(&mut self.nodes[node].children);
        for c in old {
            self.nodes[c].parent = None;
        }
        if !text.is_empty() {
            let t = self.create_text(text);
            self.append_child(node, t);
        }
    }

    pub fn id_attr(&self, node: NodeId) -> Option<&str> {
        self.element(node).and_then(|e| e.id())
    }

    pub fn classes(&self, node: NodeId) -> Vec<&str> {
        self.element(node).map(|e| e.class_list()).unwrap_or_default()
    }

    pub fn sibling_index(&self, node: NodeId) -> usize {
        if let Some(p) = self.nodes[node].parent {
            self.nodes[p]
                .children
                .iter()
                .position(|&c| c == node)
                .unwrap_or(0)
        } else {
            0
        }
    }

    pub fn prev_element_sibling(&self, node: NodeId) -> Option<NodeId> {
        let p = self.nodes[node].parent?;
        let idx = self.nodes[p].children.iter().position(|&c| c == node)?;
        self.nodes[p].children[..idx]
            .iter()
            .rev()
            .copied()
            .find(|&c| self.is_element(c))
    }

    pub fn next_element_sibling(&self, node: NodeId) -> Option<NodeId> {
        let p = self.nodes[node].parent?;
        let idx = self.nodes[p].children.iter().position(|&c| c == node)?;
        self.nodes[p].children[idx + 1..]
            .iter()
            .copied()
            .find(|&c| self.is_element(c))
    }

    pub fn element_children(&self, node: NodeId) -> Vec<NodeId> {
        self.nodes[node]
            .children
            .iter()
            .copied()
            .filter(|&c| self.is_element(c))
            .collect()
    }

    pub fn find_by_id(&self, id: &str) -> Option<NodeId> {
        self.walk(self.root).into_iter().find(|&n| {
            self.element(n).map(|e| e.id() == Some(id)).unwrap_or(false)
        })
    }

    pub fn walk(&self, start: NodeId) -> Vec<NodeId> {
        let mut out = Vec::new();
        let mut stack = vec![start];
        while let Some(n) = stack.pop() {
            out.push(n);
            for &c in self.nodes[n].children.iter().rev() {
                stack.push(c);
            }
        }
        out
    }

    /// First ancestor (including self) with the given tag.
    pub fn ancestor_tag(&self, node: NodeId, tag: &str) -> Option<NodeId> {
        let mut cur = Some(node);
        while let Some(n) = cur {
            if self.tag(n) == Some(tag) {
                return Some(n);
            }
            cur = self.nodes[n].parent;
        }
        None
    }

    pub fn ancestor_attrs(&self, node: NodeId) -> Vec<NodeId> {
        let mut out = Vec::new();
        let mut cur = self.nodes[node].parent;
        while let Some(n) = cur {
            if self.is_element(n) {
                out.push(n);
            }
            cur = self.nodes[n].parent;
        }
        out
    }

    pub fn inner_html(&self, node: NodeId) -> String {
        let mut s = String::new();
        for &c in &self.nodes[node].children {
            self.serialize_node(c, &mut s);
        }
        s
    }

    pub fn outer_html(&self, node: NodeId) -> String {
        let mut s = String::new();
        self.serialize_node(node, &mut s);
        s
    }

    pub fn serialize_node(&self, node: NodeId, out: &mut String) {
        match &self.nodes[node].data {
            NodeData::Text(t) => out.push_str(&escape_html(t)),
            NodeData::Comment(c) => {
                out.push_str("<!--");
                out.push_str(c);
                out.push_str("-->");
            }
            NodeData::Doctype(d) => {
                out.push_str("<!DOCTYPE ");
                out.push_str(d);
                out.push('>');
            }
            NodeData::Document => {
                for &c in &self.nodes[node].children {
                    self.serialize_node(c, out);
                }
            }
            NodeData::Element(e) => {
                out.push('<');
                out.push_str(&e.tag);
                for (n, v) in &e.attrs {
                    out.push(' ');
                    out.push_str(n);
                    out.push_str("=\"");
                    out.push_str(&escape_html(v));
                    out.push('"');
                }
                out.push('>');
                if is_void_tag(&e.tag) {
                    return;
                }
                for &c in &self.nodes[node].children {
                    self.serialize_node(c, out);
                }
                out.push_str("</");
                out.push_str(&e.tag);
                out.push('>');
            }
        }
    }

    pub fn deep_clone(&mut self, node: NodeId) -> NodeId {
        let data = self.nodes[node].data.clone();
        let id = self.add_node(data);
        let children = self.nodes[node].children.clone();
        for c in children {
            let cc = self.deep_clone(c);
            self.append_child(id, cc);
        }
        id
    }

    pub fn title(&self) -> String {
        for n in self.walk(self.root) {
            if self.tag(n) == Some("title") {
                return self.text_content(n).trim().to_string();
            }
        }
        String::new()
    }
}

pub fn is_void_tag(tag: &str) -> bool {
    matches!(
        tag,
        "area" | "base" | "br" | "col" | "embed" | "hr" | "img" | "input" | "link"
            | "meta" | "param" | "source" | "track" | "wbr"
    )
}

pub fn is_block_tag(tag: &str) -> bool {
    matches!(
        tag,
        "html" | "body" | "div" | "p" | "h1" | "h2" | "h3" | "h4" | "h5" | "h6"
            | "ul" | "ol" | "li" | "dl" | "dt" | "dd" | "header" | "footer" | "section"
            | "article" | "aside" | "nav" | "main" | "figure" | "figcaption" | "form"
            | "fieldset" | "legend" | "blockquote" | "pre" | "hr" | "address" | "details"
            | "summary" | "table" | "thead" | "tbody" | "tfoot" | "tr" | "td" | "th"
            | "caption" | "center" | "menu" | "dir" | "noscript" | "template" | "iframe"
    )
}
