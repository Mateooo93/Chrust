#![allow(dead_code)]
//! Layout engine: builds a box tree and performs block, inline, table and flex
//! layout with real text measurement.

use std::collections::HashMap;

use crate::css::Length;
use crate::dom::{Document, NodeId};
use crate::fonts::FontStore;
use crate::style::{
    transform_text, AlignItems, BorderStyle, ComputedStyle, Display, FlexDirection, Float,
    JustifyContent, ListStyleType, Position, TextAlign, VerticalAlign, WhiteSpace,
};
use crate::util::Color;

#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Rect {
    pub x: f32,
    pub y: f32,
    pub w: f32,
    pub h: f32,
}

impl Rect {
    pub fn new(x: f32, y: f32, w: f32, h: f32) -> Rect {
        Rect { x, y, w, h }
    }
    pub fn right(&self) -> f32 {
        self.x + self.w
    }
    pub fn bottom(&self) -> f32 {
        self.y + self.h
    }
    pub fn contains(&self, x: f32, y: f32) -> bool {
        x >= self.x && x <= self.x + self.w && y >= self.y && y <= self.y + self.h
    }
    pub fn intersects(&self, other: &Rect) -> bool {
        self.x < other.x + other.w && self.right() > other.x && self.y < other.y + other.h && self.bottom() > other.y
    }
    pub fn intersect(&self, other: &Rect) -> Rect {
        let x1 = self.x.max(other.x);
        let y1 = self.y.max(other.y);
        let x2 = self.right().min(other.right());
        let y2 = self.bottom().min(other.bottom());
        Rect { x: x1, y: y1, w: (x2 - x1).max(0.0), h: (y2 - y1).max(0.0) }
    }
}

#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Edges {
    pub top: f32,
    pub right: f32,
    pub bottom: f32,
    pub left: f32,
}

#[derive(Clone, Debug)]
pub struct ImageData {
    pub width: u32,
    pub height: u32,
    pub pixels: Vec<u8>,
    pub url: String,
}

#[derive(Clone, Debug)]
pub enum BoxKind {
    Block,
    Inline,
    Text(String),
    Image,
    Input { input_type: String, value: String, placeholder: String, checked: bool },
    Textarea { value: String, placeholder: String },
    Select { value: String },
    Break,
}

impl BoxKind {
    pub fn is_replaced(&self) -> bool {
        matches!(
            self,
            BoxKind::Image | BoxKind::Input { .. } | BoxKind::Textarea { .. } | BoxKind::Select { .. }
        )
    }
    pub fn is_text(&self) -> bool {
        matches!(self, BoxKind::Text(_))
    }
}

#[derive(Clone, Debug)]
pub struct LineFragment {
    pub text: String,
    pub node: NodeId,
    pub style: ComputedStyle,
    pub x: f32,
    pub y: f32,
    pub width: f32,
    pub height: f32,
    pub baseline: f32,
    pub atomic: bool,
    pub is_space: bool,
    pub atom_node: NodeId,
    pub valign: VerticalAlign,
}

#[derive(Clone, Debug)]
pub struct LineBox {
    pub x: f32,
    pub y: f32,
    pub width: f32,
    pub height: f32,
    pub baseline: f32,
    pub fragments: Vec<LineFragment>,
}

#[derive(Clone, Debug)]
pub struct LayoutBox {
    pub node: NodeId,
    pub style: ComputedStyle,
    pub kind: BoxKind,
    pub children: Vec<LayoutBox>,
    pub rect: Rect,
    pub content_rect: Rect,
    pub margin: Edges,
    pub padding: Edges,
    pub border: Edges,
    pub lines: Vec<LineBox>,
    pub marker: Option<LineFragment>,
    pub image: Option<ImageData>,
    pub bg_image: Option<ImageData>,
    pub mask_image: Option<ImageData>,
}

impl LayoutBox {
    pub fn new(node: NodeId, style: ComputedStyle, kind: BoxKind) -> LayoutBox {
        LayoutBox {
            node,
            style,
            kind,
            children: Vec::new(),
            rect: Rect::default(),
            content_rect: Rect::default(),
            margin: Edges::default(),
            padding: Edges::default(),
            border: Edges::default(),
            lines: Vec::new(),
            marker: None,
            image: None,
            bg_image: None,
            mask_image: None,
        }
    }

    pub fn is_block_level(&self) -> bool {
        self.style.display.is_block_level()
    }

    pub fn is_inline_level(&self) -> bool {
        if matches!(self.kind, BoxKind::Text(_) | BoxKind::Break) {
            return true;
        }
        if matches!(
            self.kind,
            BoxKind::Image | BoxKind::Input { .. } | BoxKind::Textarea { .. } | BoxKind::Select { .. }
        ) {
            return true;
        }
        self.style.display.is_inline_level()
    }

    pub fn is_replaced(&self) -> bool {
        self.kind.is_replaced()
    }

    pub fn shift(&mut self, dx: f32, dy: f32) {
        self.rect.x += dx;
        self.rect.y += dy;
        self.content_rect.x += dx;
        self.content_rect.y += dy;
        for line in &mut self.lines {
            line.x += dx;
            line.y += dy;
            line.baseline += dy;
            for f in &mut line.fragments {
                f.x += dx;
                f.y += dy;
                f.baseline += dy;
            }
        }
        if let Some(m) = &mut self.marker {
            m.x += dx;
            m.y += dy;
            m.baseline += dy;
        }
        for c in &mut self.children {
            c.shift(dx, dy);
        }
    }

    pub fn find_node_ref(&self, node: NodeId) -> Option<&LayoutBox> {
        if self.node == node && !matches!(self.kind, BoxKind::Text(_)) {
            return Some(self);
        }
        for c in &self.children {
            if let Some(f) = c.find_node_ref(node) {
                return Some(f);
            }
        }
        None
    }

    pub fn find_node_mut(&mut self, node: NodeId) -> Option<&mut LayoutBox> {
        if self.node == node && !matches!(self.kind, BoxKind::Text(_)) {
            return Some(self);
        }
        for c in &mut self.children {
            if let Some(f) = c.find_node_mut(node) {
                return Some(f);
            }
        }
        None
    }
}

pub struct LayoutResult {
    pub root: LayoutBox,
    pub page_height: f32,
    pub page_width: f32,
}

#[derive(Clone, Copy, Debug)]
pub struct FloatInfo {
    pub x: f32,
    pub y: f32,
    pub w: f32,
    pub h: f32,
    pub side: Float,
}

fn make_pseudo_box(node: NodeId, p: &crate::style::PseudoContent) -> Option<LayoutBox> {
    // Icon fonts and other private-use glyphs would render as tofu; skip them.
    if p.content
        .chars()
        .any(|c| ('\u{E000}'..='\u{F8FF}').contains(&c) || c == '\u{FFFD}')
    {
        return None;
    }
    let style = (*p.style).clone();
    if !matches!(style.display, Display::Inline | Display::InlineBlock) {
        return None;
    }
    let text_style = (*p.style).clone();
    let kind = if style.display == Display::Inline {
        BoxKind::Text(p.content.clone())
    } else {
        BoxKind::Block
    };
    let mut b = LayoutBox::new(node, style, kind);
    if !matches!(b.kind, BoxKind::Text(_)) {
        b.children.push(LayoutBox::new(node, text_style, BoxKind::Text(p.content.clone())));
    }
    Some(b)
}

pub struct LayoutEngine<'a> {
    pub doc: &'a Document,
    pub styles: &'a [ComputedStyle],
    pub fonts: &'a mut FontStore,
    pub images: &'a HashMap<String, ImageData>,
    pub viewport: (f32, f32),
    pub base_url: String,
    floats: Vec<FloatInfo>,
}

struct FlowState {
    cursor: f32,
    prev_margin: f32,
    had_block: bool,
    saw_content: bool,
}

impl<'a> LayoutEngine<'a> {
    pub fn new(
        doc: &'a Document,
        styles: &'a [ComputedStyle],
        fonts: &'a mut FontStore,
        images: &'a HashMap<String, ImageData>,
        viewport: (f32, f32),
        base_url: &str,
    ) -> LayoutEngine<'a> {
        LayoutEngine { doc, styles, fonts, images, viewport, base_url: base_url.to_string(), floats: Vec::new() }
    }

    pub fn build(&mut self, node: NodeId) -> Option<LayoutBox> {
        let style = self.styles.get(node)?.clone();
        if style.display == Display::None {
            return None;
        }
        if !self.doc.is_element(node) {
            let kind = match &self.doc.nodes[node].data {
                crate::dom::NodeData::Text(t) => BoxKind::Text(t.clone()),
                _ => return None,
            };
            return Some(LayoutBox::new(node, style, kind));
        }
        let el = self.doc.element(node).unwrap();
        let tag = el.tag.as_str().to_string();
        let mut bstyle = style.clone();
        // Table parts are laid out using block/inline machinery.
        if matches!(tag.as_str(), "thead" | "tbody" | "tfoot" | "tr" | "td" | "th" | "caption") {
            bstyle.display = match tag.as_str() {
                "td" | "th" => Display::InlineBlock,
                _ => Display::Block,
            };
        }
        let kind = match tag.as_str() {
            "img" | "svg" => BoxKind::Image,
            "input" => {
                let itype = el.attr("type").unwrap_or("text").to_ascii_lowercase();
                BoxKind::Input {
                    input_type: itype,
                    value: el.attr("value").unwrap_or("").to_string(),
                    placeholder: el.attr("placeholder").unwrap_or("").to_string(),
                    checked: el.attr("checked").is_some(),
                }
            }
            "textarea" => BoxKind::Textarea {
                value: self.doc.text_content(node),
                placeholder: el.attr("placeholder").unwrap_or("").to_string(),
            },
            "select" => {
                let mut val = String::new();
                for &c in &self.doc.nodes[node].children {
                    if self.doc.tag(c) == Some("option") {
                        let selected = self.doc.attr(c, "selected").is_some();
                        if selected || (val.is_empty() && self.doc.attr(c, "value").is_none()) {
                            let t = self.doc.text_content(c).trim().to_string();
                            val = if t.is_empty() {
                                self.doc.attr(c, "value").unwrap_or("").to_string()
                            } else {
                                t
                            };
                            if selected {
                                break;
                            }
                        }
                    }
                }
                BoxKind::Select { value: val }
            }
            "br" => BoxKind::Break,
            _ => BoxKind::Block,
        };
        let is_inline = bstyle.display == Display::Inline;
        let kind = match kind {
            BoxKind::Image
            | BoxKind::Input { .. }
            | BoxKind::Textarea { .. }
            | BoxKind::Select { .. }
            | BoxKind::Break => kind,
            _ => {
                if is_inline {
                    BoxKind::Inline
                } else {
                    BoxKind::Block
                }
            }
        };

        let mut b = LayoutBox::new(node, bstyle, kind);
        if tag == "svg" {
            let markup = self.doc.outer_html(node);
            if let Ok(img) = crate::net::render_svg_markup(&markup, "inline-svg") {
                b.image = Some(img);
            }
        }
        // Resolve image source.
        if tag == "img" && matches!(b.kind, BoxKind::Image) {
            let src = el
                .attr("src")
                .or_else(|| el.attr("data-src"))
                .or_else(|| el.attr("data-original"))
                .filter(|s| !s.trim().is_empty())
                .map(|s| s.to_string())
                .or_else(|| {
                    el.attr("srcset")
                        .or_else(|| el.attr("data-srcset"))
                        .and_then(crate::util::srcset_first)
                })
                .unwrap_or_default();
            if !src.is_empty() {
                let abs = crate::net::resolve_url(&self.base_url, src.trim());
                let _found = self.images.get(&abs).is_some();
                if let Some(data) = self.images.get(&abs) {
                    b.image = Some(data.clone());
                }
            }
        }
        if let Some(bg) = &b.style.background_image {
            let abs = crate::net::resolve_url(&self.base_url, bg);
            if let Some(data) = self.images.get(&abs) {
                b.bg_image = Some(data.clone());
            }
        }
        if let Some(mask) = &b.style.mask_image {
            let abs = crate::net::resolve_url(&self.base_url, mask);
            if let Some(data) = self.images.get(&abs) {
                b.mask_image = Some(data.clone());
            }
        }

        if b.style.display == Display::None || matches!(b.kind, BoxKind::Image | BoxKind::Input { .. } | BoxKind::Textarea { .. } | BoxKind::Select { .. } | BoxKind::Break) {
            return Some(b);
        }
        if tag == "select" {
            return Some(b);
        }
        if tag == "noscript" {
            return Some(b);
        }
        let children = self.doc.nodes[node].children.clone();
        let details_closed = tag == "details" && el.attr("open").is_none();
        let pseudo_ok = !matches!(
            b.style.display,
            Display::Flex | Display::InlineFlex | Display::Grid | Display::InlineGrid
        );
        let before = if pseudo_ok { style.before.as_ref().and_then(|p| make_pseudo_box(node, p)) } else { None };
        let after = if pseudo_ok { style.after.as_ref().and_then(|p| make_pseudo_box(node, p)) } else { None };
        if let Some(pb) = before {
            b.children.push(pb);
        }
        for c in children {
            if details_closed && self.doc.tag(c) != Some("summary") {
                continue;
            }
            if let Some(cb) = self.build(c) {
                b.children.push(cb);
            }
        }
        if let Some(pb) = after {
            b.children.push(pb);
        }
        Some(b)
    }

    pub fn layout_document(&mut self) -> LayoutResult {
        let root_node = self
            .doc
            .walk(self.doc.root)
            .into_iter()
            .find(|&n| self.doc.tag(n) == Some("html"))
            .or_else(|| {
                self.doc.nodes[self.doc.root]
                    .children
                    .iter()
                    .copied()
                    .find(|&n| self.doc.is_element(n))
            });
        let width = self.viewport.0;
        let mut root = match root_node.and_then(|n| self.build(n)) {
            Some(b) => b,
            None => LayoutBox::new(self.doc.root, ComputedStyle::default(), BoxKind::Block),
        };
        // Find the body's background to propagate, if any.
        if let Some(body) = self
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
        }
        self.layout_block(&mut root, 0.0, 0.0, width, false);
        let mut content_bottom = root.rect.y + root.rect.h + root.margin.bottom;
        max_content_bottom(&root, &mut content_bottom, false);
        let page_height = content_bottom.max(self.viewport.1);
        LayoutResult { root, page_height, page_width: width }
    }

    fn layout_block(
        &mut self,
        b: &mut LayoutBox,
        containing_x: f32,
        border_y: f32,
        available_width: f32,
        atomic: bool,
    ) {
        let avail = available_width.max(0.0);
        let style = b.style.clone();
        let basis = avail;
        let m = &style.margin;
        let p = &style.padding;
        let resolve = |l: Length, basis: f32| crate::css::resolve_length(l, basis).unwrap_or(0.0);

        let margin_left = resolve(m[3], basis);
        let margin_right = resolve(m[1], basis);
        let margin_top = resolve(m[0], basis);
        let margin_bottom = resolve(m[2], basis);
        let pad_left = resolve(p[3], basis);
        let pad_right = resolve(p[1], basis);
        let pad_top = resolve(p[0], basis);
        let pad_bottom = resolve(p[2], basis);
        let border_w = |i: usize| -> f32 {
            if style.border_style[i] == BorderStyle::None || style.border_style[i] == BorderStyle::Double && style.border_width[i] < 1.0 {
                0.0
            } else {
                style.border_width[i]
            }
        };
        let bl = border_w(3);
        let br = border_w(1);
        let bt = border_w(0);
        let bb = border_w(2);
        b.margin = Edges { top: margin_top, right: margin_right, bottom: margin_bottom, left: margin_left };
        b.padding = Edges { top: pad_top, right: pad_right, bottom: pad_bottom, left: pad_left };
        b.border = Edges { top: bt, right: br, bottom: bb, left: bl };

        let extra_h = pad_left + pad_right + bl + br;
        let mut content_w: f32;
        let intrinsic = self.intrinsic_width(b);
        match style.width {
            Length::Px(v) => {
                content_w = if style.box_sizing_border { (v - extra_h).max(0.0) } else { v.max(0.0) };
            }
            Length::Percent(pct) => {
                let v = avail * pct;
                content_w = if style.box_sizing_border { (v - extra_h).max(0.0) } else { v.max(0.0) };
            }
            Length::Auto => {
                if b.is_replaced() {
                    content_w = intrinsic.max(0.0);
                } else if atomic || style.display.is_inline_level() {
                    content_w = intrinsic.min(avail - margin_left - margin_right).max(0.0);
                } else {
                    content_w = (avail - margin_left - margin_right - extra_h).max(0.0);
                }
            }
        }
        // Clamp by min/max width.
        if let Some(v) = crate::css::resolve_length(style.min_width, basis) {
            content_w = content_w.max(v - if style.box_sizing_border { extra_h } else { 0.0 });
        }
        if let Some(v) = crate::css::resolve_length(style.max_width, basis) {
            content_w = content_w.min((v - if style.box_sizing_border { extra_h } else { 0.0 }).max(0.0));
        }
        content_w = content_w.max(0.0);

        // Horizontal placement, with auto-margin centering.
        let mut x = containing_x + margin_left;
        let ml_auto = m[3] == Length::Auto;
        let mr_auto = m[1] == Length::Auto;
        if !b.is_replaced() {
            if ml_auto && mr_auto {
                let free = avail - (content_w + extra_h);
                if free > 0.0 {
                    x = containing_x + free / 2.0;
                }
            } else if ml_auto {
                let free = avail - (content_w + extra_h + margin_right);
                if free > 0.0 {
                    x = containing_x + free;
                }
            }
        }

        let content_x = x + bl + pad_left;
        let content_y = border_y + bt + pad_top;
        b.rect = Rect::new(x, border_y, content_w + extra_h, bt + pad_top + pad_bottom + bb);
        b.content_rect = Rect::new(content_x, content_y, content_w, 0.0);

        let mut forced_h: Option<f32> = None;
        if b.is_replaced() || matches!(b.kind, BoxKind::Input { .. } | BoxKind::Textarea { .. } | BoxKind::Select { .. }) {
            let (iw, ih) = self.replaced_intrinsic_size(b);
            let mut h = match style.height {
                Length::Px(v) => {
                    if style.box_sizing_border { (v - (bt + pad_top + pad_bottom + bb)).max(0.0) } else { v.max(0.0) }
                }
                Length::Percent(pct) => self
                    .height_basis_for(b.node)
                    .map(|h| h * pct)
                    .unwrap_or(ih),
                Length::Auto => ih,
            };
            if b.is_replaced() && style.height.is_auto() && !style.width.is_auto() {
                // Preserve aspect ratio when only width is set.
                if iw > 0.0 && ih > 0.0 {
                    h = content_w * ih / iw;
                }
            }
            if !matches!(b.kind, BoxKind::Image) && style.height.is_auto() {
                h = ih;
            }
            let hb = self.height_basis_for(b.node);
            if let Some(v) = resolve_height_len(style.min_height, hb) {
                h = h.max(v);
            }
            if let Some(v) = resolve_height_len(style.max_height, hb) {
                h = h.min(v);
            }
            forced_h = Some(h.max(0.0));
        }

        let inner_h = if let Some(h) = forced_h {
            h
        } else {
            self.layout_children(b, content_x, content_y, content_w)
        };
        let mut content_h = inner_h;
        let hb = self.height_basis_for(b.node);
        let height_value = match style.height {
            Length::Px(v) => Some(v),
            Length::Percent(p) => hb.map(|h| h * p),
            Length::Auto => None,
        };
        if let Some(v) = height_value {
            let target = if style.box_sizing_border {
                (v - (bt + pad_top + pad_bottom + bb)).max(0.0)
            } else {
                v
            };
            content_h = target;
        }
        if let Some(v) = resolve_height_len(style.min_height, hb) {
            content_h = content_h.max(v);
        }
        if let Some(v) = resolve_height_len(style.max_height, hb) {
            content_h = content_h.min(v);
        }
        content_h = content_h.max(0.0);
        b.content_rect.h = content_h;
        b.rect.h = content_h + bt + pad_top + pad_bottom + bb;

        self.position_absolute_children(b);

        // Relative positioning.
        if matches!(style.position, Position::Relative) {
            let left = style.custom.get("_inset-left").and_then(|v| v.parse::<f32>().ok());
            let top = style.custom.get("_inset-top").and_then(|v| v.parse::<f32>().ok());
            let right = style.custom.get("_inset-right").and_then(|v| v.parse::<f32>().ok());
            let bottom = style.custom.get("_inset-bottom").and_then(|v| v.parse::<f32>().ok());
            let dx = left.or_else(|| right.map(|r| -r)).unwrap_or(0.0);
            let dy = top.or_else(|| bottom.map(|v| -v)).unwrap_or(0.0);
            if dx != 0.0 || dy != 0.0 {
                b.shift(dx, dy);
            }
        }

        // List marker.
        if style.display == Display::ListItem {
            self.make_list_marker(b);
        }
    }

    fn layout_children(&mut self, b: &mut LayoutBox, cx: f32, cy: f32, inner_w: f32) -> f32 {
        if b.is_replaced() {
            return b.content_rect.h;
        }
        let mut inner_w = inner_w;
    if matches!(b.style.display, Display::Grid | Display::InlineGrid) {
            return self.layout_grid(b, cx, cy, inner_w);
        }
        if b.style.display == Display::Table {
            let cols = self.table_columns(b);
            let sum: f32 = cols.iter().sum();
            let can_shrink = b.style.width.is_auto();
            if can_shrink && sum > 1.0 && sum < inner_w {
                let extra = b.padding.left + b.padding.right + b.border.left + b.border.right;
                b.content_rect.w = sum;
                b.rect.w = sum + extra;
                inner_w = sum;
            }
            self.prepare_table(b, inner_w);
        }
        let has_flex = matches!(b.style.display, Display::Flex | Display::InlineFlex);
        let element_only = b.children.iter().all(|c| {
            !matches!(c.kind, BoxKind::Text(_) | BoxKind::Break)
                || (matches!(c.kind, BoxKind::Text(_)) && match &c.kind {
                    BoxKind::Text(t) => t.trim().is_empty(),
                    _ => true,
                })
        });
        if has_flex && element_only && !b.children.is_empty() {
            return self.layout_flex(b, cx, cy, inner_w);
        }
        self.layout_flow(b, cx, cy, inner_w)
    }

    fn position_absolute_children(&mut self, b: &mut LayoutBox) {
        let cb = b.content_rect;
        for i in 0..b.children.len() {
            if !matches!(
                b.children[i].style.position,
                Position::Absolute | Position::Fixed
            ) {
                continue;
            }
            let mut child = std::mem::replace(
                &mut b.children[i],
                LayoutBox::new(b.node, ComputedStyle::default(), BoxKind::Block),
            );
            let left = child.style.custom.get("_inset-left").and_then(|v| v.parse::<f32>().ok());
            let right = child.style.custom.get("_inset-right").and_then(|v| v.parse::<f32>().ok());
            let top = child.style.custom.get("_inset-top").and_then(|v| v.parse::<f32>().ok());
            let bottom = child.style.custom.get("_inset-bottom").and_then(|v| v.parse::<f32>().ok());
            let fixed = child.style.position == Position::Fixed;
            let (cbx, cby, cbw, cbh) = if fixed {
                (0.0, 0.0, self.viewport.0, self.viewport.1)
            } else {
                (cb.x, cb.y, cb.w, cb.h)
            };
            let mut target_x = match (left, right) {
                (Some(l), _) => cbx + l,
                (None, Some(r)) => cbx + cbw - r - child.style.width.px_or(0.0),
                _ => cbx,
            };
            if child.style.height.is_auto() {
                if let (Some(t), Some(bt)) = (top, bottom) {
                    let pt = crate::css::resolve_length(child.style.padding[0], cbw).unwrap_or(0.0);
                    let pb = crate::css::resolve_length(child.style.padding[2], cbw).unwrap_or(0.0);
                    let bt_top = if child.style.border_style[0] != BorderStyle::None { child.style.border_width[0] } else { 0.0 };
                    let bt_bot = if child.style.border_style[2] != BorderStyle::None { child.style.border_width[2] } else { 0.0 };
                    let h = (cbh - t - bt - pt - pb - bt_top - bt_bot).max(0.0);
                    child.style.height = Length::Px(h);
                }
            }
            self.layout_block(&mut child, 0.0, 0.0, cbw, true);
            let w = child.rect.w;
            if left.is_none() && right.is_some() {
                target_x = cbx + cbw - right.unwrap() - w;
            }
            let target_y = match (top, bottom) {
                (Some(t), _) => cby + t,
                (None, Some(bt)) => cby + cbh - bt - child.rect.h,
                (None, None) => cby,
            };
            let dx = target_x - child.rect.x;
            let dy = target_y - child.rect.y;
            child.shift(dx, dy);
            b.children[i] = child;
        }
    }

    fn layout_flow(&mut self, b: &mut LayoutBox, cx: f32, cy: f32, inner_w: f32) -> f32 {
        let float_mark = self.floats.len();
        let mut state = FlowState { cursor: cy, prev_margin: 0.0, had_block: false, saw_content: false };
        let mut i = 0;
        while i < b.children.len() {
            let is_abs = matches!(
                b.children[i].style.position,
                Position::Absolute | Position::Fixed
            );
            if is_abs {
                i += 1;
                continue;
            }
            if b.children[i].style.float != Float::None {
                let side = b.children[i].style.float;
                {
                    let mut child = std::mem::replace(
                        &mut b.children[i],
                        LayoutBox::new(b.node, ComputedStyle::default(), BoxKind::Block),
                    );
                    self.layout_block(&mut child, cx, state.cursor, inner_w, true);
                    let w = child.rect.w;
                    let h = child.rect.h;
                    let mut fy = state.cursor;
                    let target_x = loop {
                        let mut left = cx;
                        let mut right = cx + inner_w;
                        let mut next_y: Option<f32> = None;
                        for f in &self.floats {
                            if fy < f.y + f.h && fy + h > f.y {
                                match f.side {
                                    Float::Left => left = left.max(f.x + f.w),
                                    Float::Right => right = right.min(f.x),
                                    Float::None => {}
                                }
                                let bottom = f.y + f.h;
                                if bottom > fy {
                                    next_y = Some(next_y.map_or(bottom, |n: f32| n.min(bottom)));
                                }
                            }
                        }
                        if right - left >= w || next_y.is_none() {
                            break match side {
                                Float::Left => left,
                                Float::Right => right - w,
                                Float::None => child.rect.x,
                            };
                        }
                        fy = next_y.unwrap();
                    };
                    let dx = target_x - child.rect.x;
                    let dy = fy - child.rect.y;
                    child.shift(dx, dy);
                    self.floats.push(FloatInfo { x: child.rect.x, y: child.rect.y, w, h: child.rect.h, side });
                    b.children[i] = child;
                }
                i += 1;
                continue;
            }
            let inline = b.children[i].is_inline_level();
            if inline {
                let start = i;
                while i < b.children.len()
                    && b.children[i].is_inline_level()
                    && !matches!(b.children[i].style.position, Position::Absolute | Position::Fixed)
                {
                    i += 1;
                }
                let end = i;
                let h = self.layout_inline_run(b, start, end, cx, state.cursor, inner_w);
                if h > 0.0 {
                    state.cursor += h;
                    state.saw_content = true;
                    state.prev_margin = 0.0;
                    state.had_block = false;
                }
                continue;
            }
            // Block child.
            let mt = self.resolve_margin(b.children[i].style.margin[0], inner_w);
            let gap = if !state.had_block && !state.saw_content {
                mt
            } else if state.had_block {
                state.prev_margin.max(mt)
            } else {
                mt
            };
            state.cursor += gap;
            let child_y = state.cursor;
            {
                let mut child = std::mem::replace(
                    &mut b.children[i],
                    LayoutBox::new(b.node, ComputedStyle::default(), BoxKind::Block),
                );
                self.layout_block(&mut child, cx, child_y, inner_w, false);
                // Legacy <center> / text-align centering of tables.
                if matches!(self.doc.tag(child.node), Some("table")) {
                    let free = inner_w - child.rect.w;
                    if free > 0.5 {
                        let dx = match b.style.text_align {
                            crate::style::TextAlign::Center => free / 2.0,
                            crate::style::TextAlign::Right => free,
                            _ => 0.0,
                        };
                        if dx != 0.0 {
                            child.shift(dx, 0.0);
                        }
                    }
                }
                let mb = self.resolve_margin(child.style.margin[2], inner_w);
                state.cursor = child.rect.y + child.rect.h;
                state.prev_margin = mb;
                b.children[i] = child;
            }
            state.had_block = true;
            state.saw_content = true;
            i += 1;
        }

        let mut height = (state.cursor - cy).max(0.0);
        for f in &self.floats[float_mark.min(self.floats.len())..] {
            height = height.max(f.y + f.h - cy);
        }
        self.floats.truncate(float_mark);
        height
    }

    /// Basis for percentage heights: only explicit containing-block heights
    /// count. html gets the viewport; auto-height ancestors make percentages
    /// behave as auto.
    fn height_basis_for(&self, node: NodeId) -> Option<f32> {
        if self.doc.tag(node) == Some("html") {
            return Some(self.viewport.1);
        }
        let parent = self.doc.nodes[node].parent?;
        if !self.doc.is_element(parent) {
            return None;
        }
        self.definite_height_of(parent)
    }

    fn definite_height_of(&self, node: NodeId) -> Option<f32> {
        let tag = self.doc.tag(node).unwrap_or("");
        if tag == "html" {
            return Some(self.viewport.1);
        }
        let style = self.styles.get(node)?;
        match style.height {
            Length::Px(v) => Some(v),
            Length::Percent(p) => self.height_basis_for(node).map(|b| b * p),
            Length::Auto => None,
        }
    }

    fn horizontal_extra(&self, b: &LayoutBox, basis: f32) -> f32 {
        let pl = crate::css::resolve_length(b.style.padding[3], basis).unwrap_or(0.0);
        let pr = crate::css::resolve_length(b.style.padding[1], basis).unwrap_or(0.0);
        let bl = if b.style.border_style[3] != BorderStyle::None { b.style.border_width[3] } else { 0.0 };
        let br = if b.style.border_style[1] != BorderStyle::None { b.style.border_width[1] } else { 0.0 };
        pl + pr + bl + br
    }

    fn line_area(&self, y: f32, cx: f32, max_w: f32, indent: f32, line_h: f32) -> (f32, f32) {
        let mut left = cx + indent;
        let mut right = cx + max_w;
        for f in &self.floats {
            if y < f.y + f.h && y + line_h > f.y {
                match f.side {
                    Float::Left => left = left.max(f.x + f.w),
                    Float::Right => right = right.min(f.x),
                    Float::None => {}
                }
            }
        }
        if right - left < 24.0 {
            right = left + max_w.max(24.0);
        }
        (left, right - left)
    }

    fn resolve_margin(&self, l: Length, basis: f32) -> f32 {
        match l {
            Length::Px(v) => v,
            Length::Percent(p) => basis * p,
            Length::Auto => 0.0,
        }
    }

    // ---------------------------------------------------------- inline layout

    fn layout_inline_run(
        &mut self,
        b: &mut LayoutBox,
        start: usize,
        end: usize,
        cx: f32,
        y: f32,
        max_w: f32,
    ) -> f32 {
        let mut tokens: Vec<InlineToken> = Vec::new();
        let mut last_space = true;
        for i in start..end {
            let mut child = std::mem::replace(
                &mut b.children[i],
                LayoutBox::new(b.node, ComputedStyle::default(), BoxKind::Block),
            );
            self.collect_inline(&mut child, &mut tokens, &mut last_space, max_w);
            b.children[i] = child;
        }
        if tokens.is_empty() {
            return 0.0;
        }
        let first_line = b.lines.is_empty();
        let total_h = self.build_lines(b, tokens, cx, y, max_w, first_line);
        total_h
    }

    fn collect_inline(
        &mut self,
        child: &mut LayoutBox,
        tokens: &mut Vec<InlineToken>,
        last_space: &mut bool,
        avail: f32,
    ) {
        match &child.kind {
            BoxKind::Text(text) => {
                self.tokenize_text(text, child, tokens, last_space);
            }
            BoxKind::Break => {
                tokens.push(InlineToken::Break);
                *last_space = true;
            }
            BoxKind::Image | BoxKind::Input { .. } | BoxKind::Textarea { .. } | BoxKind::Select { .. } => {
                self.layout_block(child, 0.0, 0.0, avail, true);
                let w = child.rect.w;
                let h = child.rect.h;
                tokens.push(InlineToken::Atom {
                    node: child.node,
                    width: w,
                    height: h,
                    style: child.style.clone(),
                    valign: child.style.vertical_align,
                });
                *last_space = false;
            }
            _ => {
                if child.style.display == Display::InlineBlock
                    || child.style.display == Display::InlineFlex
                    || child.style.display == Display::InlineGrid
                    || child.style.display == Display::Inline
                {
                    if child.style.display == Display::Inline {
                        // Recurse into inline element children.
                        for c in &mut child.children {
                            self.collect_inline(c, tokens, last_space, avail);
                        }
                    } else {
                        let saved = std::mem::take(&mut self.floats);
                        self.layout_block(child, 0.0, 0.0, avail, true);
                        self.floats = saved;
                        let w = child.rect.w;
                        let h = child.rect.h;
                        tokens.push(InlineToken::Atom {
                            node: child.node,
                            width: w,
                            height: h,
                            style: child.style.clone(),
                            valign: child.style.vertical_align,
                        });
                        *last_space = false;
                    }
                } else {
                    // Block content inside an inline context: treat as an atomic box.
                    let saved = std::mem::take(&mut self.floats);
                    self.layout_block(child, 0.0, 0.0, avail, true);
                    self.floats = saved;
                    let w = child.rect.w;
                    let h = child.rect.h;
                    tokens.push(InlineToken::Atom {
                        node: child.node,
                        width: w,
                        height: h,
                        style: child.style.clone(),
                        valign: child.style.vertical_align,
                    });
                    *last_space = false;
                }
            }
        }
    }

    fn tokenize_text(
        &mut self,
        text: &str,
        child: &LayoutBox,
        tokens: &mut Vec<InlineToken>,
        last_space: &mut bool,
    ) {
        let style = child.style.clone();
        let text = transform_text(text, style.text_transform);
        let ws = style.white_space;
        if ws == WhiteSpace::Pre || ws == WhiteSpace::PreWrap {
            let chars: Vec<char> = text.chars().collect();
            let mut word = String::new();
            let flush = |word: &mut String, tokens: &mut Vec<InlineToken>| {
                if !word.is_empty() {
                    tokens.push(InlineToken::Word {
                        text: std::mem::take(word),
                        style: style.clone(),
                        node: child.node,
                    });
                }
            };
            for c in chars {
                match c {
                    '\n' => {
                        flush(&mut word, tokens);
                        tokens.push(InlineToken::Break);
                    }
                    ' ' => {
                        flush(&mut word, tokens);
                        tokens.push(InlineToken::Space { style: style.clone(), node: child.node });
                    }
                    '\t' => {
                        flush(&mut word, tokens);
                        for _ in 0..4 {
                            tokens.push(InlineToken::Space { style: style.clone(), node: child.node });
                        }
                    }
                    '\r' => {}
                    _ => word.push(c),
                }
            }
            flush(&mut word, tokens);
            *last_space = false;
            return;
        }
        // Collapse whitespace, treating NBSP as a normal character.
        let mut normalized = String::new();
        for c in text.chars() {
            if c == '\n' || c == '\t' || c == '\r' || c == ' ' {
                normalized.push(' ');
            } else if c == '\u{00A0}' {
                normalized.push('\u{00A0}');
            } else {
                normalized.push(c);
            }
        }
        if ws == WhiteSpace::PreLine {
            // Newlines still break lines; done by splitting below.
        }
        let mut word = String::new();
        for c in normalized.chars() {
            if c == ' ' {
                if !word.is_empty() {
                    tokens.push(InlineToken::Word {
                        text: std::mem::take(&mut word),
                        style: style.clone(),
                        node: child.node,
                    });
                    *last_space = false;
                }
                if !*last_space {
                    tokens.push(InlineToken::Space { style: style.clone(), node: child.node });
                    *last_space = true;
                }
            } else if ws == WhiteSpace::PreLine && c == '\n' {
                if !word.is_empty() {
                    tokens.push(InlineToken::Word {
                        text: std::mem::take(&mut word),
                        style: style.clone(),
                        node: child.node,
                    });
                }
                tokens.push(InlineToken::Break);
                *last_space = true;
            } else {
                word.push(c);
                *last_space = false;
            }
        }
        if !word.is_empty() {
            tokens.push(InlineToken::Word { text: word, style, node: child.node });
            *last_space = false;
        }
    }

    fn build_lines(
        &mut self,
        b: &mut LayoutBox,
        tokens: Vec<InlineToken>,
        cx: f32,
        cy: f32,
        max_w: f32,
        first_line: bool,
    ) -> f32 {
        let base_style = b.style.clone();
        let mut y = cy;
        let start_y = y;
        let mut frags: Vec<LineFragment> = Vec::new();
        let mut line_w = 0.0f32;
        let mut indent = if first_line { base_style.text_indent } else { 0.0 };
        let mut area = self.line_area(y, cx, max_w, indent, base_style.line_height);

        let mut idx = 0;
        while idx < tokens.len() {
            match &tokens[idx] {
                InlineToken::Break => {
                    if frags.is_empty() {
                        let lh = base_style.line_height.max(1.0);
                        let (asc, _) = self.fonts.line_metrics(&base_style);
                        b.lines.push(LineBox {
                            x: area.0,
                            y,
                            width: 0.0,
                            height: lh,
                            baseline: y + asc,
                            fragments: Vec::new(),
                        });
                        y += lh;
                    } else {
                        self.finish_line(b, &mut frags, area, &mut y, &base_style);
                    }
                    line_w = 0.0;
                    indent = 0.0;
                    area = self.line_area(y, cx, max_w, indent, base_style.line_height);
                    idx += 1;
                    continue;
                }
                InlineToken::Space { style, node } => {
                    let pre = matches!(style.white_space, WhiteSpace::Pre | WhiteSpace::PreWrap);
                    if frags.is_empty() && !pre {
                        idx += 1;
                        continue;
                    }
                    let w = self.fonts.char_advance(' ', style);
                    let can_wrap = !matches!(style.white_space, WhiteSpace::Nowrap | WhiteSpace::Pre);
                    if can_wrap && !frags.is_empty() && line_w + w > area.1 {
                        self.finish_line(b, &mut frags, area, &mut y, &base_style);
                        line_w = 0.0;
                        indent = 0.0;
                        area = self.line_area(y, cx, max_w, indent, base_style.line_height);
                        idx += 1;
                        continue;
                    }
                    frags.push(LineFragment {
                        text: " ".to_string(),
                        node: *node,
                        style: style.clone(),
                        x: area.0 + line_w,
                        y,
                        width: w,
                        height: style.line_height,
                        baseline: y,
                        atomic: false,
                        is_space: true,
                        atom_node: 0,
                        valign: VerticalAlign::Baseline,
                    });
                    line_w += w;
                    idx += 1;
                    continue;
                }
                InlineToken::Word { text, style, node } => {
                    let word = text.clone();
                    let style = style.clone();
                    let node = *node;
                    let can_wrap = !matches!(style.white_space, WhiteSpace::Nowrap | WhiteSpace::Pre);
                    let w = self.fonts.measure_style(&word, &style);
                    if can_wrap && !frags.is_empty() && line_w + w > area.1 {
                        self.finish_line(b, &mut frags, area, &mut y, &base_style);
                        line_w = 0.0;
                        indent = 0.0;
                        area = self.line_area(y, cx, max_w, indent, base_style.line_height);
                    }
                    if can_wrap && w > area.1 && area.1 > 1.0 {
                        let chars: Vec<char> = word.chars().collect();
                        let mut chunk = String::new();
                        let mut chunk_w = 0.0;
                        for ch in chars.iter() {
                            let adv = self.fonts.char_advance(*ch, &style);
                            if !frags.is_empty() && line_w + chunk_w + adv > area.1 {
                                if !chunk.is_empty() {
                                    let cw = chunk_w;
                                    frags.push(LineFragment {
                                        text: std::mem::take(&mut chunk),
                                        node,
                                        style: style.clone(),
                                        x: area.0 + line_w,
                                        y,
                                        width: cw,
                                        height: style.line_height,
                                        baseline: y,
                                        atomic: false,
                                        is_space: false,
                                        atom_node: 0,
                                        valign: VerticalAlign::Baseline,
                                    });
                                    line_w += cw;
                                    chunk_w = 0.0;
                                }
                                self.finish_line(b, &mut frags, area, &mut y, &base_style);
                                line_w = 0.0;
                                indent = 0.0;
                                area = self.line_area(y, cx, max_w, indent, base_style.line_height);
                            }
                            chunk.push(*ch);
                            chunk_w += adv;
                        }
                        if !chunk.is_empty() {
                            frags.push(LineFragment {
                                text: chunk,
                                node,
                                style: style.clone(),
                                x: area.0 + line_w,
                                y,
                                width: chunk_w,
                                height: style.line_height,
                                baseline: y,
                                atomic: false,
                                is_space: false,
                                atom_node: 0,
                                valign: VerticalAlign::Baseline,
                            });
                            line_w += chunk_w;
                        }
                    } else {
                        frags.push(LineFragment {
                            text: word,
                            node,
                            style: style.clone(),
                            x: area.0 + line_w,
                            y,
                            width: w,
                            height: style.line_height,
                            baseline: y,
                            atomic: false,
                            is_space: false,
                            atom_node: 0,
                            valign: VerticalAlign::Baseline,
                        });
                        line_w += w;
                    }
                    idx += 1;
                    continue;
                }
                InlineToken::Atom { node, width, height, style, valign } => {
                    let node = *node;
                    let w = *width;
                    let h = *height;
                    let valign = *valign;
                    let can_wrap = !matches!(style.white_space, WhiteSpace::Nowrap);
                    if can_wrap && !frags.is_empty() && line_w + w > area.1 {
                        self.finish_line(b, &mut frags, area, &mut y, &base_style);
                        line_w = 0.0;
                        indent = 0.0;
                        area = self.line_area(y, cx, max_w, indent, base_style.line_height);
                    }
                    frags.push(LineFragment {
                        text: String::new(),
                        node,
                        style: style.clone(),
                        x: area.0 + line_w,
                        y,
                        width: w,
                        height: h,
                        baseline: y,
                        atomic: true,
                        is_space: false,
                        atom_node: node,
                        valign,
                    });
                    line_w += w;
                    idx += 1;
                    continue;
                }
            }
        }
        if !frags.is_empty() {
            self.finish_line(b, &mut frags, area, &mut y, &base_style);
        }
        y - start_y
    }

    fn finish_line(
        &mut self,
        b: &mut LayoutBox,
        frags: &mut Vec<LineFragment>,
        area: (f32, f32),
        y: &mut f32,
        base: &ComputedStyle,
    ) {
        while frags
            .last()
            .map(|f| f.is_space && !f.atomic && f.text.trim().is_empty())
            .unwrap_or(false)
        {
            frags.pop();
        }
        if frags.is_empty() {
            return;
        }
        let mut line_height = base.line_height.max(1.0);
        let mut max_ascent: f32 = 0.0;
        let mut max_descent: f32 = 0.0;
        let mut content_right: f32 = 0.0;
        let mut min_x = f32::MAX;
        for f in frags.iter() {
            if f.atomic {
                line_height = line_height.max(f.height);
                max_ascent = max_ascent.max(f.height);
            } else {
                let (asc, desc) = self.fonts.line_metrics(&f.style);
                line_height = line_height.max(f.style.line_height);
                let half = (f.style.line_height - (asc + desc)).max(0.0) / 2.0;
                max_ascent = max_ascent.max(asc + half);
                max_descent = max_descent.max(desc + half);
            }
            content_right = content_right.max(f.x + f.width);
            min_x = min_x.min(f.x);
        }
        if min_x == f32::MAX {
            min_x = area.0;
        }
        let mut lh = line_height.max(max_ascent + max_descent);
        let baseline_rel = max_ascent.max(lh * 0.8);
        lh = lh.max(baseline_rel + max_descent);
        let baseline = *y + baseline_rel;
        let line_content_w = (content_right - min_x).max(0.0);
        let offset = match base.text_align {
            TextAlign::Center => ((area.1 - line_content_w) / 2.0).max(0.0),
            TextAlign::Right => (area.1 - line_content_w).max(0.0),
            _ => 0.0,
        };
        let target_origin = area.0 + offset;
        let dx = target_origin - min_x;
        for f in frags.iter_mut() {
            let atom_h = f.height;
            f.x += dx;
            f.y = *y;
            f.height = lh;
            f.baseline = baseline;
            if f.atomic {
                let ah = atom_h.min(lh);
                let ay = match f.valign {
                    VerticalAlign::Top | VerticalAlign::TextTop => *y,
                    VerticalAlign::Middle => *y + (lh - ah) / 2.0,
                    VerticalAlign::Bottom | VerticalAlign::TextBottom => *y + lh - ah,
                    _ => baseline - ah,
                };
                f.y = ay;
            }
        }
        let line = LineBox {
            x: area.0,
            y: *y,
            width: line_content_w,
            height: lh,
            baseline,
            fragments: frags.clone(),
        };
        for f in &line.fragments {
            if f.atomic {
                if let Some(atom) = b.find_node_mut(f.atom_node) {
                    let dxa = f.x - atom.rect.x;
                    let dya = f.y - atom.rect.y;
                    atom.shift(dxa, dya);
                }
            }
        }
        b.lines.push(line);
        *y += lh;
        frags.clear();
    }

    // ------------------------------------------------------------- replaced

    fn replaced_intrinsic_size(&mut self, b: &mut LayoutBox) -> (f32, f32) {
        match &b.kind {
            BoxKind::Image => {
                if let Some(img) = &b.image {
                    (img.width as f32, img.height as f32)
                } else {
                    (24.0, 24.0)
                }
            }
            BoxKind::Input { input_type, .. } => {
                if input_type == "checkbox" || input_type == "radio" {
                    (13.0, 13.0)
                } else if input_type == "submit" || input_type == "button" || input_type == "reset" {
                    (90.0, 24.0)
                } else {
                    (170.0, 21.0)
                }
            }
            BoxKind::Textarea { .. } => (170.0, 60.0),
            BoxKind::Select { .. } => (120.0, 21.0),
            _ => (0.0, 0.0),
        }
    }

    // ------------------------------------------------------------- intrinsic

    fn intrinsic_width(&mut self, b: &LayoutBox) -> f32 {
        let (pl, pr) = (
            crate::css::resolve_length(b.style.padding[3], 100.0).unwrap_or(0.0),
            crate::css::resolve_length(b.style.padding[1], 100.0).unwrap_or(0.0),
        );
        let extra = pl + pr + b.style.border_width[1] + b.style.border_width[3];
        if let Some(v) = crate::css::resolve_length(b.style.width, 1000.0) {
            return v + extra;
        }
        if b.is_replaced() {
            let (iw, _ih) = match &b.kind {
                BoxKind::Image => b.image.as_ref().map(|i| (i.width as f32, i.height as f32)).unwrap_or((24.0, 24.0)),
                BoxKind::Input { input_type, .. } => {
                    if input_type == "checkbox" || input_type == "radio" {
                        (13.0, 13.0)
                    } else if input_type == "submit" || input_type == "button" || input_type == "reset" {
                        (90.0, 24.0)
                    } else {
                        (170.0, 21.0)
                    }
                }
                BoxKind::Textarea { .. } => (170.0, 60.0),
                BoxKind::Select { .. } => (120.0, 21.0),
                _ => (0.0, 0.0),
            };
            return iw + extra;
        }
        if let Some(v) = crate::css::resolve_length(b.style.width, 1000.0) {
            return v + extra;
        }
        match &b.kind {
            BoxKind::Text(text) => {
                let mut w: f32 = 0.0;
                for line in text.split('\n') {
                    w = w.max(self.fonts.measure_style(line, &b.style));
                }
                w
            }
            BoxKind::Image => 24.0,
            BoxKind::Break => 0.0,
            BoxKind::Inline => {
                if b.style.display == Display::Flex {
                    return 0.0;
                }
                let mut sum = 0.0;
                for c in &b.children {
                    sum += self.intrinsic_width(c);
                }
                sum + extra
            }
            _ => {
    if matches!(b.style.display, Display::Grid | Display::InlineGrid) {
                    let mut maxc: f32 = 0.0;
                    for c in &b.children {
                        maxc = maxc.max(self.intrinsic_width(c));
                    }
                    return maxc + extra;
                }
                if matches!(b.style.display, Display::Flex | Display::InlineFlex) {
                    let row = matches!(b.style.flex_direction, FlexDirection::Row | FlexDirection::RowReverse);
                    let mut total = 0.0;
                    let mut maxc: f32 = 0.0;
                    for c in &b.children {
                        let iw = self.intrinsic_width(c);
                        if row {
                            total += iw;
                        } else {
                            maxc = maxc.max(iw);
                        }
                    }
                    return if row { total } else { maxc } + extra;
                }
                let mut maxc: f32 = 0.0;
                for c in &b.children {
                    maxc = maxc.max(self.intrinsic_width(c));
                }
                maxc + extra
            }
        }
    }

    // ------------------------------------------------------------------ flex

    fn layout_flex(&mut self, b: &mut LayoutBox, cx: f32, cy: f32, inner_w: f32) -> f32 {
        // Flex items are blockified.
        for child in &mut b.children {
            if child.style.display == Display::Inline {
                child.style.display = Display::Block;
            } else if child.style.display == Display::InlineFlex {
                child.style.display = Display::Flex;
            }
        }
        let items: Vec<usize> = b
            .children
            .iter()
            .enumerate()
            .filter(|(_, c)| !matches!(c.kind, BoxKind::Text(_) | BoxKind::Break))
            .map(|(i, _)| i)
            .collect();
        let n = items.len();
        if n == 0 {
            return self.layout_flow(b, cx, cy, inner_w);
        }
        let is_row = matches!(b.style.flex_direction, FlexDirection::Row | FlexDirection::RowReverse);
        let reverse = matches!(b.style.flex_direction, FlexDirection::RowReverse | FlexDirection::ColumnReverse);
        let gap = b.style.gap.max(0.0);
        let n_gaps = n.saturating_sub(1) as f32;
        if is_row && b.style.flex_wrap {
            return self.layout_flex_wrap_row(b, cx, cy, inner_w, &items);
        }
        if is_row {
            let mut widths: Vec<f32> = Vec::with_capacity(n);
            for &i in &items {
                let child = &b.children[i];
                let extra = self.horizontal_extra(child, inner_w);
                let basis = match child.style.flex_basis {
                    Length::Px(v) => v + if child.style.box_sizing_border { 0.0 } else { extra },
                    Length::Percent(p) => inner_w * p,
                    Length::Auto => {
                        if let Some(v) = crate::css::resolve_length(child.style.width, inner_w) {
                            v + if child.style.box_sizing_border { 0.0 } else { extra }
                        } else {
                            self.intrinsic_width(child).min(inner_w)
                        }
                    }
                };
                widths.push(basis.max(0.0));
            }
            let total: f32 = widths.iter().sum::<f32>() + gap * n_gaps;
            let mut free = inner_w - total;
            if free > 0.0 {
                let grow_total: f32 = items.iter().map(|&i| b.children[i].style.flex_grow).sum();
                if grow_total > 0.0 {
                    for (k, &i) in items.iter().enumerate() {
                        widths[k] += free * b.children[i].style.flex_grow / grow_total;
                    }
                    free = 0.0;
                }
            } else if free < 0.0 {
                let shrink_total: f32 = items
                    .iter()
                    .enumerate()
                    .map(|(k, &i)| b.children[i].style.flex_shrink * widths[k])
                    .sum();
                if shrink_total > 0.0 {
                    for (k, &i) in items.iter().enumerate() {
                        widths[k] -= (-free) * b.children[i].style.flex_shrink * widths[k] / shrink_total;
                        widths[k] = widths[k].max(0.0);
                    }
                }
            }
            let used: f32 = widths.iter().sum::<f32>() + gap * n_gaps;
            let leftover = (inner_w - used).max(0.0);
            let ml_auto: Vec<bool> = items
                .iter()
                .map(|&i| b.children[i].style.margin[3] == Length::Auto)
                .collect();
            let mr_auto: Vec<bool> = items
                .iter()
                .map(|&i| b.children[i].style.margin[1] == Length::Auto)
                .collect();
            let auto_count = ml_auto.iter().chain(mr_auto.iter()).filter(|a| **a).count();
            let auto_unit = if auto_count > 0 { leftover / auto_count as f32 } else { 0.0 };
            let (mut x, extra_gap) = if auto_count > 0 {
                (cx, 0.0)
            } else { match b.style.justify_content {
                JustifyContent::Center => (cx + leftover / 2.0, 0.0),
                JustifyContent::End => (cx + leftover, 0.0),
                JustifyContent::SpaceBetween => {
                    (cx, if n > 1 { leftover / (n as f32 - 1.0) } else { 0.0 })
                }
                JustifyContent::SpaceAround => {
                    let unit = leftover / n as f32;
                    (cx + unit / 2.0, unit)
                }
                JustifyContent::SpaceEvenly => {
                    let unit = leftover / (n as f32 + 1.0);
                    (cx + unit, unit)
                }
                JustifyContent::Start => (cx, 0.0),
            } };
            let mut heights: Vec<f32> = vec![0.0; n];
            let order: Vec<usize> = if reverse { (0..n).rev().collect() } else { (0..n).collect() };
            let mut placements: Vec<(usize, f32)> = Vec::new();
            for &k in &order {
                let i = items[k];
                if auto_count > 0 {
                    if ml_auto[k] {
                        x += auto_unit;
                    }
                }
                let outer_w = widths[k].max(0.0);
                let extra = self.horizontal_extra(&b.children[i], inner_w);
                let content_w = (outer_w - extra).max(1.0);
                {
                    let mut child = std::mem::replace(
                        &mut b.children[i],
                        LayoutBox::new(b.node, ComputedStyle::default(), BoxKind::Block),
                    );
                    if child.style.box_sizing_border {
                        child.style.width = Length::Px(outer_w);
                    } else {
                        child.style.width = Length::Px(content_w);
                    }
                    // Percent min/max sizes resolve against the flex container.
                    if let Length::Percent(p) = child.style.max_width {
                        child.style.max_width = Length::Px(inner_w * p);
                    }
                    if let Length::Percent(p) = child.style.min_width {
                        child.style.min_width = Length::Px(inner_w * p);
                    }
                    self.layout_block(&mut child, x, cy, content_w, false);
                    heights[k] = child.rect.h;
                    b.children[i] = child;
                }
                placements.push((k, x));
                x += outer_w + gap + extra_gap;
                if auto_count > 0 && mr_auto[k] {
                    x += auto_unit;
                }
            }
            let mut cross = 0.0f32;
            for h in &heights {
                cross = cross.max(*h);
            }
            if let Length::Px(h) = b.style.height {
                cross = h;
            } else if let Length::Percent(p) = b.style.height {
                if let Some(h) = self.height_basis_for(b.node) {
                    cross = h * p;
                }
            }
            for (k, _px) in placements {
                let i = items[k];
                let ch = heights[k];
                let mut y = cy;
                match b.children[i].style.align_self {
                    AlignItems::Stretch => {
                        if b.children[i].style.height.is_auto() {
                            let target_h = cross.max(ch);
                            let delta = target_h - b.children[i].rect.h;
                            if delta > 0.0 {
                                b.children[i].rect.h += delta;
                                b.children[i].content_rect.h += delta;
                            }
                        }
                    }
                    AlignItems::Center => y = cy + (cross - ch) / 2.0,
                    AlignItems::End => y = cy + cross - ch,
                    _ => {}
                }
                let dy = y - b.children[i].rect.y;
                b.children[i].shift(0.0, dy);
            }
            cross
        } else {
            // Column flex.
            let mut heights: Vec<f32> = Vec::with_capacity(n);
            for &i in &items {
                let extra = self.horizontal_extra(&b.children[i], inner_w);
                let border_box = b.children[i].style.box_sizing_border;
                let outer_w = match b.children[i].style.width {
                    Length::Px(v) => {
                        if border_box { v } else { v + extra }
                    }
                    Length::Percent(p) => {
                        if border_box { inner_w * p } else { inner_w * p + extra }
                    }
                    Length::Auto => inner_w,
                };
                let content_w = (outer_w - extra).max(1.0);
                let mut cb = std::mem::replace(
                    &mut b.children[i],
                    LayoutBox::new(b.node, ComputedStyle::default(), BoxKind::Block),
                );
                cb.style.width = Length::Px(if border_box { outer_w } else { content_w });
                self.layout_block(&mut cb, cx, cy, content_w, false);
                heights.push(cb.rect.h);
                b.children[i] = cb;
            }
            let total: f32 = heights.iter().sum::<f32>() + gap * n_gaps;
            let hb = self.height_basis_for(b.node);
            let explicit_h = match b.style.height {
                Length::Px(v) => Some(v),
                Length::Percent(p) => hb.map(|h| h * p),
                Length::Auto => None,
            };
            let container_h = explicit_h.unwrap_or(total.max(0.0));
            let mut extra = (container_h - total).max(0.0);
            if extra > 0.0 {
                let grow_total: f32 = items.iter().map(|&i| b.children[i].style.flex_grow).sum();
                if grow_total > 0.0 {
                    for (k, &i) in items.iter().enumerate() {
                        heights[k] += extra * b.children[i].style.flex_grow / grow_total;
                    }
                    extra = 0.0;
                }
            }
            let (y0, extra_gap) = match b.style.justify_content {
                JustifyContent::Center => (cy + extra / 2.0, 0.0),
                JustifyContent::End => (cy + extra, 0.0),
                JustifyContent::SpaceBetween => (cy, if n > 1 { extra / (n as f32 - 1.0) } else { 0.0 }),
                JustifyContent::SpaceAround => (cy + extra / n as f32 / 2.0, extra / n as f32),
                JustifyContent::SpaceEvenly => (cy + extra / (n as f32 + 1.0), extra / (n as f32 + 1.0)),
                JustifyContent::Start => (cy, 0.0),
            };
            let order: Vec<usize> = if reverse { (0..n).rev().collect() } else { (0..n).collect() };
            let mut target_y = y0;
            for &k in &order {
                let i = items[k];
                let target_h = heights[k];
                let child_h = b.children[i].rect.h;
                if target_h > child_h {
                    b.children[i].rect.h += target_h - child_h;
                    b.children[i].content_rect.h += target_h - child_h;
                }
                let mut x = cx;
                match b.children[i].style.align_self {
                    AlignItems::Center => x = cx + (inner_w - b.children[i].rect.w) / 2.0,
                    AlignItems::End => x = cx + inner_w - b.children[i].rect.w,
                    _ => {}
                }
                let dx = x - b.children[i].rect.x;
                let dy = target_y - b.children[i].rect.y;
                b.children[i].shift(dx, dy);
                target_y += target_h + gap + extra_gap;
            }
            container_h
        }
    }

    // ------------------------------------------------------------------ grid

    fn layout_grid(&mut self, b: &mut LayoutBox, cx: f32, cy: f32, inner_w: f32) -> f32 {
        for child in &mut b.children {
            if child.style.display == Display::Inline {
                child.style.display = Display::Block;
            }
        }
        // Grid items are element children; absolutely positioned ones are laid separately.
        let items: Vec<usize> = b
            .children
            .iter()
            .enumerate()
            .filter(|(_, c)| {
                !matches!(c.kind, BoxKind::Text(_) | BoxKind::Break)
                    && !matches!(c.style.position, Position::Absolute | Position::Fixed)
            })
            .map(|(i, _)| i)
            .collect();
        if items.is_empty() {
            return 0.0;
        }
        let areas = b.style.grid_template_areas.clone();
        let col_tracks = parse_grid_tracks(&b.style.grid_template_columns, inner_w, 16.0);
        let mut row_tracks = parse_grid_tracks(&b.style.grid_template_rows, inner_w, 16.0);
        let auto_rows = parse_grid_tracks(&b.style.grid_auto_rows, inner_w, 16.0);
        let area_cols = areas.iter().map(|r| r.len()).max().unwrap_or(0);
        let mut ncols = col_tracks.len().max(area_cols).max(1);
        let mut nrows = row_tracks.len().max(areas.len()).max(1);
        if !auto_rows.is_empty() {
            if row_tracks.is_empty() {
                row_tracks.push(auto_rows[0]);
            }
            while row_tracks.len() < 128 {
                row_tracks.push(*auto_rows.last().unwrap());
            }
        }
        // Column/row placements. Definite placements are applied first so
        // auto-placed items flow around them.
        let mut placements: Vec<(usize, usize, usize, usize, usize)> = Vec::new();
        let mut definite: Vec<Option<(usize, usize, usize, usize, usize)>> = vec![None; items.len()];
        for (k, &i) in items.iter().enumerate() {
            let child = &b.children[i];
            let mut placed = None;
            if !child.style.grid_area.is_empty() {
                if let Some((r0, c0, r1, c1)) = find_grid_area(&areas, &child.style.grid_area) {
                    placed = Some((i, r0, c0, (r1 - r0).max(1), (c1 - c0).max(1)));
                }
            }
            if placed.is_none() && (!child.style.grid_column.is_empty() || !child.style.grid_row.is_empty()) {
                let (mut start, mut span) = parse_grid_line(&child.style.grid_column);
                let (mut row_start, mut row_span) = parse_grid_line(&child.style.grid_row);
                if start.is_none() && span.is_none() && !child.style.grid_column.is_empty() {
                    let names = grid_line_names(&b.style.grid_template_columns);
                    let r = resolve_named_line(&child.style.grid_column, &names);
                    start = r.0;
                    span = r.1;
                }
                if row_start.is_none() && row_span.is_none() && !child.style.grid_row.is_empty() {
                    let names = grid_line_names(&b.style.grid_template_rows);
                    let r = resolve_named_line(&child.style.grid_row, &names);
                    row_start = r.0;
                    row_span = r.1;
                }
                placed = Some((
                    i,
                    row_start.unwrap_or(0),
                    start.unwrap_or(0),
                    row_span.unwrap_or(1),
                    span.unwrap_or(1),
                ));
            }
            if let Some(p) = placed {
                definite[k] = Some(p);
                ncols = ncols.max(p.2 + p.4);
                nrows = nrows.max(p.1 + p.3);
                placements.push(p);
            }
        }
        let mut cursor_row = 0usize;
        let mut cursor_col = 0usize;
        for (k, &i) in items.iter().enumerate() {
            if definite[k].is_some() {
                continue;
            }
            let mut r = cursor_row;
            let mut c = cursor_col;
            loop {
                if c >= ncols {
                    c = 0;
                    r += 1;
                }
                let taken = placements.iter().any(|&(_, pr, pc, prs, pcs)| {
                    r < pr + prs && r >= pr && c < pc + pcs && c >= pc
                });
                if !taken {
                    break;
                }
                c += 1;
            }
            cursor_row = r;
            cursor_col = c + 1;
            ncols = ncols.max(c + 1);
            nrows = nrows.max(r + 1);
            placements.push((i, r, c, 1, 1));
        }
        let col_gap = b.style.gap.max(0.0);
        let row_gap = b.style.row_gap.max(0.0);
        // Column widths.
        let mut col_widths = vec![0.0f32; ncols];
        let mut fr_total = 0.0f32;
        let mut content_cols: Vec<usize> = Vec::new();
        for c in 0..ncols {
            if let Some(track) = col_tracks.get(c) {
                match track.kind {
                    TrackKind::Px => col_widths[c] = track.base,
                    TrackKind::Fr => {
                        col_widths[c] = track.base;
                        fr_total += track.fr;
                    }
                    TrackKind::Content => content_cols.push(c),
                }
            } else {
                content_cols.push(c);
            }
        }
        // Content columns get the intrinsic width of their items.
        for c in content_cols.clone() {
            let mut w = 1.0f32;
            for &(i, _r, pc, _rs, cs) in &placements {
                if pc == c && cs == 1 {
                    w = w.max(self.intrinsic_width(&b.children[i]));
                }
            }
            col_widths[c] = w;
        }
        let mut fixed: f32 = col_widths.iter().sum::<f32>() + col_gap * (ncols.saturating_sub(1)) as f32;
        if fixed > inner_w && fr_total == 0.0 && content_cols.len() == ncols && ncols > 1 {
            let avail = (inner_w - col_gap * (ncols.saturating_sub(1)) as f32).max(1.0);
            let each = avail / ncols as f32;
            for w in col_widths.iter_mut() {
                *w = each;
            }
            fixed = inner_w;
        }
        if fixed > inner_w && fixed > 1.0 {
            let scale = ((inner_w - col_gap * (ncols.saturating_sub(1)) as f32).max(1.0)) / fixed;
            for w in col_widths.iter_mut() {
                *w = (*w * scale).max(1.0);
            }
            fixed = col_widths.iter().sum::<f32>() + col_gap * (ncols.saturating_sub(1)) as f32;
        }
        let free = (inner_w - fixed).max(0.0);
        if fr_total > 0.0 {
            let unit = free / fr_total;
            for c in 0..ncols {
                if let Some(track) = col_tracks.get(c) {
                    if track.kind == TrackKind::Fr {
                        col_widths[c] += unit * track.fr;
                        if let Some(max) = track.max {
                            col_widths[c] = col_widths[c].min(max);
                        }
                    }
                }
            }
        } else if !content_cols.is_empty() && free > 0.0 {
            let extra = free / content_cols.len() as f32;
            for c in &content_cols {
                col_widths[*c] += extra;
            }
        }
        if std::env::var("AURORA_DEBUG_GRID").is_ok() {
            eprintln!(
                "grid {} ncols={} cols={:?} inner_w={}",
                self.doc.attr(b.node, "class").unwrap_or(""),
                ncols,
                col_widths,
                inner_w
            );
        }
        let col_x = |c: usize, widths: &[f32]| -> f32 {
            let mut x = cx;
            for k in 0..c.min(widths.len()) {
                x += widths[k] + col_gap;
            }
            x
        };
        // Lay out items at the top of the grid; y offsets are applied later.
        let mut item_heights: Vec<f32> = vec![0.0; items.len()];
        for (k, &(i, _r, c, _rs, cs)) in placements.iter().enumerate() {
            let x = col_x(c, &col_widths);
            let mut w = 0.0;
            for cc in c..(c + cs).min(ncols) {
                w += col_widths[cc];
                if cc > c {
                    w += col_gap;
                }
            }
            {
                let mut child = std::mem::replace(
                    &mut b.children[i],
                    LayoutBox::new(b.node, ComputedStyle::default(), BoxKind::Block),
                );
                if child.style.width.is_auto() {
                    let extra = self.horizontal_extra(&child, w);
                    child.style.width = Length::Px(if child.style.box_sizing_border {
                        w.max(1.0)
                    } else {
                        (w - extra).max(1.0)
                    });
                }
                self.layout_block(&mut child, x, cy, w.max(1.0), false);
                item_heights[k] = child.rect.h;
                b.children[i] = child;
            }
        }
        // Row heights.
        let mut row_heights = vec![0.0f32; nrows];
        let mut row_fr = vec![0.0f32; nrows];
        for r in 0..nrows {
            if let Some(track) = row_tracks.get(r) {
                match track.kind {
                    TrackKind::Px => row_heights[r] = track.base,
                    TrackKind::Fr => row_fr[r] = track.fr,
                    TrackKind::Content => {}
                }
            }
        }
        for (k, &(_i, r, _c, rs, _cs)) in placements.iter().enumerate() {
            let h = item_heights[k];
            if rs == 1 {
                row_heights[r] = row_heights[r].max(h);
            } else {
                let share = h / rs as f32;
                for rr in r..(r + rs).min(nrows) {
                    row_heights[rr] = row_heights[rr].max(share);
                }
            }
        }
        let gap_h = row_gap * (nrows.saturating_sub(1)) as f32;
        let mut total_h: f32 = row_heights.iter().sum::<f32>() + gap_h;
        let explicit_h = match b.style.height {
            Length::Px(v) => Some(v),
            Length::Percent(p) => self.height_basis_for(b.node).map(|h| h * p),
            Length::Auto => None,
        };
        if let Some(h) = explicit_h {
            let extra = (h - total_h).max(0.0);
            let fr_total: f32 = row_fr.iter().sum();
            if fr_total > 0.0 && extra > 0.0 {
                for r in 0..nrows {
                    row_heights[r] += extra * row_fr[r] / fr_total;
                }
                total_h = h;
            }
        }
        // Position items, stretching auto-height items across their row span.
        for (k, &(i, r, _c, rs, _cs)) in placements.iter().enumerate() {
            let mut y = cy;
            for rr in 0..r {
                y += row_heights[rr] + row_gap;
            }
            let mut span_h = 0.0;
            for rr in r..(r + rs).min(nrows) {
                span_h += row_heights[rr];
            }
            if rs > 1 {
                span_h += row_gap * (rs.saturating_sub(1)) as f32;
            }
            let child = &mut b.children[i];
            if child.style.height.is_auto()
                && !matches!(
                    child.style.align_self,
                    AlignItems::Center | AlignItems::Start | AlignItems::End
                )
                && span_h > child.rect.h
            {
                let delta = span_h - child.rect.h;
                child.rect.h += delta;
                child.content_rect.h += delta;
            }
            let dy = y - child.rect.y;
            if dy != 0.0 {
                child.shift(0.0, dy);
            }
            let _ = k;
        }
        total_h.max(0.0)
    }

    fn layout_flex_wrap_row(
        &mut self,
        b: &mut LayoutBox,
        cx: f32,
        cy: f32,
        inner_w: f32,
        items: &[usize],
    ) -> f32 {
        let gap = b.style.gap.max(0.0);
        let row_gap = b.style.row_gap.max(gap);
        let mut base: Vec<f32> = Vec::with_capacity(items.len());
        for &i in items {
            let child = &b.children[i];
            let extra = self.horizontal_extra(child, inner_w);
            let basis = match child.style.flex_basis {
                Length::Px(v) => v + if child.style.box_sizing_border { 0.0 } else { extra },
                Length::Percent(p) => inner_w * p,
                Length::Auto => {
                    if let Some(v) = crate::css::resolve_length(child.style.width, inner_w) {
                        v + if child.style.box_sizing_border { 0.0 } else { extra }
                    } else {
                        self.intrinsic_width(child).min(inner_w)
                    }
                }
            };
            base.push(basis.max(0.0));
        }
        // Partition into lines.
        let mut lines: Vec<Vec<usize>> = Vec::new();
        let mut line: Vec<usize> = Vec::new();
        let mut line_w = 0.0f32;
        for (k, &_i) in items.iter().enumerate() {
            let w = base[k];
            let add = if line.is_empty() { w } else { gap + w };
            if !line.is_empty() && line_w + add > inner_w + 0.5 {
                lines.push(std::mem::take(&mut line));
                line_w = 0.0;
            }
            if !line.is_empty() {
                line_w += gap;
            }
            line.push(k);
            line_w += w;
        }
        if !line.is_empty() {
            lines.push(line);
        }
        let reverse = b.style.flex_direction == FlexDirection::RowReverse;
        let mut y = cy;
        for line in lines {
            let mut widths: Vec<f32> = line.iter().map(|&k| base[k]).collect();
            let n = line.len();
            let used: f32 = widths.iter().sum::<f32>() + gap * (n.saturating_sub(1)) as f32;
            let free = inner_w - used;
            if free > 0.0 {
                let grow_total: f32 = line
                    .iter()
                    .map(|&k| b.children[items[k]].style.flex_grow)
                    .sum();
                if grow_total > 0.0 {
                    for (j, &k) in line.iter().enumerate() {
                        widths[j] += free * b.children[items[k]].style.flex_grow / grow_total;
                    }
                }
            } else if free < 0.0 {
                let shrink_total: f32 = line
                    .iter()
                    .enumerate()
                    .map(|(j, &k)| b.children[items[k]].style.flex_shrink * widths[j])
                    .sum();
                if shrink_total > 0.0 {
                    for (j, &k) in line.iter().enumerate() {
                        widths[j] -= (-free) * b.children[items[k]].style.flex_shrink * widths[j] / shrink_total;
                        widths[j] = widths[j].max(0.0);
                    }
                }
            }
            let used_now: f32 = widths.iter().sum::<f32>() + gap * (n.saturating_sub(1)) as f32;
            let leftover = (inner_w - used_now).max(0.0);
            let (mut x, extra_gap) = match b.style.justify_content {
                JustifyContent::Center => (cx + leftover / 2.0, 0.0),
                JustifyContent::End => (cx + leftover, 0.0),
                JustifyContent::SpaceBetween => {
                    (cx, if n > 1 { leftover / (n as f32 - 1.0) } else { 0.0 })
                }
                JustifyContent::SpaceAround => {
                    let unit = leftover / n as f32;
                    (cx + unit / 2.0, unit)
                }
                JustifyContent::SpaceEvenly => {
                    let unit = leftover / (n as f32 + 1.0);
                    (cx + unit, unit)
                }
                JustifyContent::Start => (cx, 0.0),
            };
            let order: Vec<usize> = if reverse { (0..n).rev().collect() } else { (0..n).collect() };
            let mut heights: Vec<f32> = vec![0.0; n];
            let mut placements: Vec<(usize, f32)> = Vec::new();
            for &j in &order {
                let k = line[j];
                let i = items[k];
                let outer_w = widths[j].max(0.0);
                let extra = self.horizontal_extra(&b.children[i], inner_w);
                let content_w = (outer_w - extra).max(1.0);
                {
                    let mut child = std::mem::replace(
                        &mut b.children[i],
                        LayoutBox::new(b.node, ComputedStyle::default(), BoxKind::Block),
                    );
                    child.style.width = Length::Px(content_w);
                    self.layout_block(&mut child, x, y, content_w, false);
                    heights[j] = child.rect.h;
                    b.children[i] = child;
                }
                placements.push((j, x));
                x += outer_w + gap + extra_gap;
            }
            let mut cross = 0.0f32;
            for h in &heights {
                cross = cross.max(*h);
            }
            for (j, _px) in placements {
                let i = items[line[j]];
                let ch = heights[j];
                let mut target_y = y;
                match b.children[i].style.align_self {
                    AlignItems::Center => target_y = y + (cross - ch) / 2.0,
                    AlignItems::End => target_y = y + cross - ch,
                    AlignItems::Stretch => {
                        if b.children[i].style.height.is_auto() {
                            let delta = cross.max(ch) - b.children[i].rect.h;
                            if delta > 0.0 {
                                b.children[i].rect.h += delta;
                                b.children[i].content_rect.h += delta;
                            }
                        }
                    }
                    _ => {}
                }
                let dy = target_y - b.children[i].rect.y;
                b.children[i].shift(0.0, dy);
            }
            y += cross + row_gap;
        }
        (y - cy - row_gap).max(0.0)
    }

    // ----------------------------------------------------------------- tables

    fn prepare_table(&mut self, b: &mut LayoutBox, inner_w: f32) {
        let cols = self.table_columns(b);
        let ncols = cols.len().max(1);
        let sum: f32 = cols.iter().sum();
        let mut widths = vec![0.0f32; ncols];
        if sum > 0.5 {
            let scale = inner_w / sum;
            for i in 0..ncols {
                widths[i] = (cols[i] * scale).max(1.0);
            }
        } else {
            for w in widths.iter_mut() {
                *w = inner_w / ncols as f32;
            }
        }
        let table_node = b.node;
        self.assign_cell_widths(b, &widths, table_node);
    }

    fn table_columns(&mut self, b: &LayoutBox) -> Vec<f32> {
        let mut cols: Vec<f32> = Vec::new();
        self.count_table_columns(b, &mut cols);
        cols
    }

    fn count_table_columns(&mut self, b: &LayoutBox, cols: &mut Vec<f32>) {
        for child in &b.children {
            match self.doc.tag(child.node) {
                Some("tr") => {
                    let mut col = 0usize;
                    for cell in &child.children {
                        if matches!(self.doc.tag(cell.node), Some("td") | Some("th")) {
                            let span = self
                                .doc
                                .attr(cell.node, "colspan")
                                .and_then(|v| v.parse::<usize>().ok())
                                .unwrap_or(1)
                                .max(1);
                            let desired = self.intrinsic_width(cell).max(1.0);
                            let per = desired / span as f32;
                            while cols.len() < col + span {
                                cols.push(0.0);
                            }
                            for k in 0..span {
                                cols[col + k] = cols[col + k].max(per);
                            }
                            col += span;
                        }
                    }
                }
                Some("table") => {}
                _ => self.count_table_columns(child, cols),
            }
        }
    }

    fn assign_cell_widths(&self, b: &mut LayoutBox, col_widths: &[f32], table_node: NodeId) {
        for child in &mut b.children {
            if child.node == table_node {
                continue;
            }
            match self.doc.tag(child.node) {
                Some("tr") => {
                    child.style.text_align = TextAlign::Left;
                    let mut col = 0usize;
                    for cell in &mut child.children {
                        if matches!(self.doc.tag(cell.node), Some("td") | Some("th")) {
                            let span = self
                                .doc
                                .attr(cell.node, "colspan")
                                .and_then(|v| v.parse::<usize>().ok())
                                .unwrap_or(1)
                                .max(1);
                            let mut target = 0.0;
                            for k in 0..span {
                                target += col_widths.get(col + k).copied().unwrap_or(0.0);
                            }
                            if cell.style.width.is_auto() {
                                let pad = crate::css::resolve_length(cell.style.padding[1], 100.0).unwrap_or(0.0)
                                    + crate::css::resolve_length(cell.style.padding[3], 100.0).unwrap_or(0.0)
                                    + cell.style.border_width[1]
                                    + cell.style.border_width[3];
                                cell.style.width = Length::Px((target - pad).max(1.0));
                            }
                            cell.style.vertical_align = VerticalAlign::Top;
                            cell.style.text_align = TextAlign::Left;
                            col += span;
                        }
                    }
                }
                Some("table") => {}
                _ => self.assign_cell_widths(child, col_widths, table_node),
            }
        }
    }

    // ---------------------------------------------------------------- markers

    fn make_list_marker(&mut self, b: &mut LayoutBox) {
        let style = b.style.clone();
        if style.list_style_type == ListStyleType::None {
            return;
        }
        let index = self.list_index(b.node);
        let text = match style.list_style_type {
            ListStyleType::Disc => "•".to_string(),
            ListStyleType::Circle => "◦".to_string(),
            ListStyleType::Square => "▪".to_string(),
            ListStyleType::Decimal => format!("{}.", index),
            ListStyleType::DecimalLeadingZero => {
                if index < 10 {
                    format!("0{}.", index)
                } else {
                    format!("{}.", index)
                }
            }
            ListStyleType::LowerAlpha => format!("{}.", alpha_num(index, false)),
            ListStyleType::UpperAlpha => format!("{}.", alpha_num(index, true)),
            ListStyleType::LowerRoman => format!("{}.", roman(index).to_ascii_lowercase()),
            ListStyleType::UpperRoman => format!("{}.", roman(index)),
            ListStyleType::None => return,
        };
        let mut mstyle = style.clone();
        mstyle.background_color = Color::TRANSPARENT;
        mstyle.text_decoration = Default::default();
        let w = self.fonts.measure_style(&text, &mstyle);
        let (line_y, baseline) = if let Some(line) = b.lines.first() {
            (line.y, line.baseline)
        } else {
            let (asc, _) = self.fonts.line_metrics(&mstyle);
            (b.content_rect.y, b.content_rect.y + asc)
        };
        let x = b.content_rect.x - w - 7.0;
        b.marker = Some(LineFragment {
            text,
            node: b.node,
            style: mstyle,
            x,
            y: line_y,
            width: w,
            height: style.line_height,
            baseline,
            atomic: false,
            is_space: false,
            atom_node: 0,
            valign: VerticalAlign::Baseline,
        });
    }

    fn list_index(&self, node: NodeId) -> usize {
        let mut n = 1;
        if let Some(parent) = self.doc.nodes[node].parent {
            for &c in &self.doc.nodes[parent].children {
                if c == node {
                    break;
                }
                if self.doc.tag(c) == Some("li") {
                    n += 1;
                }
            }
        }
        if let Some(v) = self.doc.attr(node, "value") {
            if let Ok(nv) = v.parse::<usize>() {
                return nv;
            }
        }
        n
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum TrackKind {
    Px,
    Fr,
    Content,
}

#[derive(Clone, Copy, Debug)]
struct GridTrack {
    kind: TrackKind,
    base: f32,
    fr: f32,
    max: Option<f32>,
}

fn parse_grid_tracks(value: &str, inner_w: f32, font_size: f32) -> Vec<GridTrack> {
    let mut out = Vec::new();
    if value.trim().is_empty() || value.trim() == "none" {
        return out;
    }
    let tokens = split_track_tokens(value);
    for token in tokens {
        let t = token.trim();
        if t.is_empty() {
            continue;
        }
        if let Some(inner) = t.strip_prefix("repeat(").and_then(|s| s.strip_suffix(')')) {
            let parts = split_top_level_str(inner, ',');
            if parts.len() == 2 {
                let rep = parts[0].trim().to_ascii_lowercase();
                let inner_tracks = parse_grid_tracks(&parts[1], inner_w, font_size);
                let count: usize = if rep == "auto-fill" || rep == "auto-fit" {
                    let gap = 0.0f32;
                    let min_track = inner_tracks
                        .iter()
                        .map(|tr| tr.base.max(1.0))
                        .fold(f32::MAX, f32::min)
                        .max(1.0);
                    if min_track.is_finite() && min_track > 1.0 {
                        ((inner_w + gap) / (min_track + gap)).floor().max(1.0) as usize
                    } else {
                        1
                    }
                } else {
                    rep.parse().unwrap_or(1)
                }
                .min(24);
                for _ in 0..count {
                    out.extend(inner_tracks.iter().copied());
                }
            }
            continue;
        }
        if let Some(inner) = t.strip_prefix("minmax(").and_then(|s| s.strip_suffix(')')) {
            let parts = split_top_level_str(inner, ',');
            if parts.len() == 2 {
                let min_s = parts[0].trim();
                let max_s = parts[1].trim();
                let min_px = parse_track_length(min_s, inner_w, font_size).unwrap_or(0.0);
                if max_s.ends_with("fr") {
                    out.push(GridTrack {
                        kind: TrackKind::Fr,
                        base: min_px,
                        fr: max_s.trim_end_matches("fr").trim().parse().unwrap_or(1.0),
                        max: None,
                    });
                    continue;
                }
                if let Some(px) = parse_track_length(max_s, inner_w, font_size) {
                    // minmax(min, fixed-max): use the maximum as a definite
                    // size, shrinking implicitly if the grid is overfull.
                    out.push(GridTrack {
                        kind: TrackKind::Px,
                        base: px.max(min_px),
                        fr: 0.0,
                        max: None,
                    });
                    continue;
                }
            }
            out.push(GridTrack { kind: TrackKind::Content, base: 0.0, fr: 0.0, max: None });
            continue;
        }
        if t.ends_with("fr") {
            let fr: f32 = t.trim_end_matches("fr").trim().parse().unwrap_or(1.0);
            out.push(GridTrack { kind: TrackKind::Fr, base: 0.0, fr, max: None });
        } else if t == "auto" || t.contains("content") || t.starts_with("fit-content") {
            out.push(GridTrack { kind: TrackKind::Content, base: 0.0, fr: 0.0, max: None });
        } else if let Some(px) = parse_track_length(t, inner_w, font_size) {
            out.push(GridTrack { kind: TrackKind::Px, base: px, fr: 0.0, max: None });
        } else {
            out.push(GridTrack { kind: TrackKind::Content, base: 0.0, fr: 0.0, max: None });
        }
    }
    out
}

fn parse_track_length(token: &str, inner_w: f32, font_size: f32) -> Option<f32> {
    let t = token.trim();
    if t == "0" {
        return Some(0.0);
    }
    for (name, kind) in [("min(", 0u8), ("max(", 1u8), ("clamp(", 2u8)] {
        if let Some(rest) = t.strip_prefix(name).and_then(|v| v.strip_suffix(')')) {
            let args: Vec<f32> = split_top_level_str(rest, ',')
                .iter()
                .filter_map(|a| parse_track_length(a, inner_w, font_size))
                .collect();
            return match kind {
                0 => args.into_iter().fold(None, |acc: Option<f32>, v| Some(acc.map_or(v, |a| a.min(v)))),
                1 => args.into_iter().fold(None, |acc: Option<f32>, v| Some(acc.map_or(v, |a| a.max(v)))),
                _ => {
                    if args.len() >= 3 {
                        Some(args[1].max(args[0]).min(args[2]))
                    } else {
                        args.first().copied()
                    }
                }
            };
        }
    }
    let len = crate::css::parse_length(t, font_size, 16.0, (inner_w, 800.0))?;
    crate::css::resolve_length(len, inner_w)
}

fn split_track_tokens(value: &str) -> Vec<String> {
    let mut out: Vec<String> = Vec::new();
    let mut cur = String::new();
    let mut depth = 0i32;
    let mut bracket = 0i32;
    let mut prev: Option<char> = None;
    for c in value.chars() {
        if bracket > 0 {
            match c {
                '[' => bracket += 1,
                ']' => bracket -= 1,
                _ => {}
            }
            prev = Some(c);
            continue;
        }
        // A function following a closing paren starts a new track.
        if depth == 0 && prev == Some(')') && (c.is_alphabetic() || c == '-') && !cur.is_empty() {
            out.push(std::mem::take(&mut cur));
        }
        match c {
            '[' => {
                let _ = std::mem::take(&mut cur);
                bracket = 1;
            }
            '(' => {
                depth += 1;
                cur.push(c);
            }
            ')' => {
                depth -= 1;
                cur.push(c);
            }
            c if c.is_whitespace() && depth == 0 => {
                if !cur.is_empty() {
                    out.push(std::mem::take(&mut cur));
                }
            }
            _ => cur.push(c),
        }
        prev = Some(c);
    }
    if !cur.is_empty() {
        out.push(cur);
    }
    out
}

fn split_top_level_str(s: &str, sep: char) -> Vec<String> {
    let mut out = Vec::new();
    let mut cur = String::new();
    let mut depth = 0i32;
    for c in s.chars() {
        match c {
            '(' => {
                depth += 1;
                cur.push(c);
            }
            ')' => {
                depth -= 1;
                cur.push(c);
            }
            c if depth == 0 && c == sep => {
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

fn find_grid_area(areas: &[Vec<String>], name: &str) -> Option<(usize, usize, usize, usize)> {
    let mut min_r = usize::MAX;
    let mut min_c = usize::MAX;
    let mut max_r = 0usize;
    let mut max_c = 0usize;
    let mut found = false;
    for (r, row) in areas.iter().enumerate() {
        for (c, cell) in row.iter().enumerate() {
            if cell == name {
                found = true;
                min_r = min_r.min(r);
                min_c = min_c.min(c);
                max_r = max_r.max(r + 1);
                max_c = max_c.max(c + 1);
            }
        }
    }
    if found {
        Some((min_r, min_c, max_r, max_c))
    } else {
        None
    }
}

fn grid_line_names(value: &str) -> std::collections::HashMap<String, usize> {
    let mut names = std::collections::HashMap::new();
    let mut line = 0usize;
    let mut in_bracket = false;
    let mut cur = String::new();
    let mut flush_line = |line: usize, cur: &mut String, names: &mut std::collections::HashMap<String, usize>| {
        for name in cur.split_whitespace() {
            names.insert(name.to_ascii_lowercase(), line);
        }
        cur.clear();
    };
    let mut depth = 0i32;
    for c in value.chars() {
        if in_bracket {
            match c {
                ']' => {
                    flush_line(line, &mut cur, &mut names);
                    in_bracket = false;
                }
                _ => cur.push(c),
            }
            continue;
        }
        match c {
            '[' => in_bracket = true,
            '(' => depth += 1,
            ')' => depth -= 1,
            _ if depth == 0 && (c.is_whitespace() || c == ',') => {
                if !cur.trim().is_empty() {
                    line += 1;
                }
                cur.clear();
            }
            _ => cur.push(c),
        }
    }
    if !cur.trim().is_empty() {
        let _ = line;
    }
    names
}

fn resolve_named_line(
    value: &str,
    names: &std::collections::HashMap<String, usize>,
) -> (Option<usize>, Option<usize>) {
    let parts: Vec<&str> = value.split('/').map(|p| p.trim()).collect();
    let lookup = |p: &str| -> Option<usize> {
        if let Some(n) = names.get(&p.to_ascii_lowercase()) {
            return Some(*n);
        }
        if p.ends_with("-start") || p.ends_with("-end") {
            if let Some(n) = names.get(&p.to_ascii_lowercase()) {
                return Some(*n);
            }
        }
        None
    };
    // A bare area-like name (`content`) may refer to the line pair
    // `[content-start] ... [content-end]`.
    let area_span = |name: &str| -> Option<(usize, usize)> {
        let low = name.to_ascii_lowercase();
        let s = names.get(&format!("{}-start", low)).copied();
        let e = names.get(&format!("{}-end", low)).copied();
        match (s, e) {
            (Some(s), Some(e)) if e > s => Some((s, e - s)),
            _ => None,
        }
    };
    match parts.as_slice() {
        [a] if !a.is_empty() => {
            if let Some(s) = lookup(a) {
                (Some(s), None)
            } else if let Some((s, span)) = area_span(a) {
                (Some(s), Some(span))
            } else {
                (None, None)
            }
        }
        [a, b, ..] => {
            if let (Some(s), Some(e)) = (lookup(a), lookup(b)) {
                let span = if e > s { Some(e - s) } else { None };
                return (Some(s), span);
            }
            if let Some((s, span)) = area_span(a) {
                return (Some(s), Some(span));
            }
            (lookup(a).or(lookup(b)), None)
        }
        _ => (None, None),
    }
}

fn parse_grid_line_part(part: &str) -> (Option<usize>, Option<usize>) {
    let p = part.trim();
    if p.is_empty() || p == "auto" {
        return (None, None);
    }
    if let Some(span) = p.strip_prefix("span ") {
        return (None, span.trim().parse::<usize>().ok());
    }
    if let Ok(n) = p.parse::<isize>() {
        if n > 0 {
            return (Some((n - 1) as usize), None);
        }
    }
    (None, None)
}

fn parse_grid_line(value: &str) -> (Option<usize>, Option<usize>) {
    let v = value.trim();
    if v.is_empty() || v == "auto" {
        return (None, None);
    }
    let parts: Vec<&str> = v.split('/').map(|p| p.trim()).collect();
    if parts.len() >= 2 {
        let (start, first_span) = parse_grid_line_part(parts[0]);
        let (other, second_span) = parse_grid_line_part(parts[1]);
        let span = first_span
            .or(second_span)
            .or_else(|| match (start, other) {
                (Some(s), Some(e)) if e > s => Some(e - s),
                _ => None,
            });
        return (start.or(other), span);
    }
    parse_grid_line_part(v)
}

fn resolve_height_len(l: Length, basis: Option<f32>) -> Option<f32> {
    match l {
        Length::Px(v) => Some(v),
        Length::Percent(p) => basis.map(|b| b * p),
        Length::Auto => None,
    }
}

fn max_content_bottom(b: &LayoutBox, max_bottom: &mut f32, inside_fixed: bool) -> f32 {
    if inside_fixed {
        return *max_bottom;
    }
    let fixed = b.style.position == Position::Fixed;
    if !fixed {
        *max_bottom = max_bottom.max(b.rect.y + b.rect.h);
        for line in &b.lines {
            for f in &line.fragments {
                *max_bottom = max_bottom.max(f.y + f.height);
            }
        }
        if let Some(m) = &b.marker {
            *max_bottom = max_bottom.max(m.y + m.height);
        }
    }
    for c in &b.children {
        max_content_bottom(c, max_bottom, fixed);
    }
    *max_bottom
}

fn kind_is_block(k: &BoxKind) -> bool {
    matches!(k, BoxKind::Block)
}

fn alpha_num(mut n: usize, upper: bool) -> String {
    if n == 0 {
        n = 1;
    }
    let mut s = String::new();
    while n > 0 {
        let rem = (n - 1) % 26;
        let base = if upper { b'A' } else { b'a' };
        s.insert(0, (base + rem as u8) as char);
        n = (n - 1) / 26;
    }
    s
}

fn roman(mut n: usize) -> String {
    let vals = [
        (1000, "M"), (900, "CM"), (500, "D"), (400, "CD"), (100, "C"), (90, "XC"),
        (50, "L"), (40, "XL"), (10, "X"), (9, "IX"), (5, "V"), (4, "IV"), (1, "I"),
    ];
    let mut out = String::new();
    for (v, s) in vals {
        while n >= v {
            out.push_str(s);
            n -= v;
        }
    }
    out
}

impl Length {
    pub fn px_or(self, default: f32) -> f32 {
        match self {
            Length::Px(v) => v,
            Length::Percent(_) => default,
            Length::Auto => default,
        }
    }
}

enum InlineToken {
    Word { text: String, style: ComputedStyle, node: NodeId },
    Space { style: ComputedStyle, node: NodeId },
    Break,
    Atom { node: NodeId, width: f32, height: f32, style: ComputedStyle, valign: VerticalAlign },
}

// Helpers referenced by an over-complicated legacy closure; keep simple stubs.
fn cx_placeholder() -> f32 {
    0.0
}
fn indent_base(_min_x: f32, _w: f32, _max_w: f32) -> f32 {
    0.0
}
fn indent_used(_frags: &[LineFragment], indent: f32) -> f32 {
    indent
}

// -------------------------------------------------------------- hit testing

pub fn hit_test(b: &LayoutBox, x: f32, y: f32) -> Option<NodeId> {
    // Atoms and block children first (later siblings paint on top).
    for child in b.children.iter().rev() {
        if child.is_inline_level() && !matches!(child.style.position, Position::Absolute | Position::Fixed) {
            continue;
        }
        if let Some(n) = hit_test(child, x, y) {
            return Some(n);
        }
    }
    for line in b.lines.iter().rev() {
        for f in line.fragments.iter().rev() {
            if f.width <= 0.0 || f.is_space {
                continue;
            }
            let rect = Rect::new(f.x, f.y, f.width, f.height.max(1.0));
            if rect.contains(x, y) {
                return Some(f.node);
            }
        }
    }
    if let Some(img) = &b.image {
        let _ = img;
        if b.rect.contains(x, y) {
            return Some(b.node);
        }
    }
    if matches!(b.kind, BoxKind::Input { .. } | BoxKind::Textarea { .. } | BoxKind::Select { .. }) {
        if b.rect.contains(x, y) {
            return Some(b.node);
        }
    }
    if b.rect.contains(x, y) {
        return Some(b.node);
    }
    None
}

/// Find the deepest box whose rect contains the point (used for cursor changes).
pub fn find_link_node(doc: &Document, node: NodeId) -> Option<NodeId> {
    let mut cur = Some(node);
    while let Some(n) = cur {
        if doc.tag(n) == Some("a") && doc.attr(n, "href").is_some() {
            return Some(n);
        }
        cur = doc.nodes[n].parent;
    }
    None
}

pub fn dump_layout(doc: &Document, b: &LayoutBox, depth: usize, out: &mut String) {
    let tag = doc.tag(b.node).unwrap_or("#text");
    let id = doc.attr(b.node, "id").unwrap_or("");
    let cls = doc.attr(b.node, "class").unwrap_or("");
    out.push_str(&format!(
        "{}{} id={} class={} display={:?} rect=({:.0},{:.0},{:.0},{:.0}) content=({:.0},{:.0},{:.0},{:.0}) lines={} kids={}\n",
        "  ".repeat(depth),
        tag,
        id,
        cls,
        b.style.display,
        b.rect.x, b.rect.y, b.rect.w, b.rect.h,
        b.content_rect.x, b.content_rect.y, b.content_rect.w, b.content_rect.h,
        b.lines.len(),
        b.children.len()
    ));
    if b.style.float != Float::None {
        out.push_str(&format!("{}  FLOAT {:?} side\n", "  ".repeat(depth), b.style.float));
    }
    if doc.tag(b.node) == Some("img") {
        out.push_str(&format!(
            "{}  IMG width={:?} height={:?} has_data={} attr_w={:?} attr_h={:?} src={:?}\n",
            "  ".repeat(depth),
            b.style.width,
            b.style.height,
            b.image.is_some(),
            doc.attr(b.node, "width"),
            doc.attr(b.node, "height"),
            doc.attr(b.node, "src")
        ));
    }
    if matches!(b.style.display, Display::Grid | Display::InlineGrid) {
        out.push_str(&format!("{}  GRID cols={:?} rows={:?} areas={:?}
", "  ".repeat(depth), b.style.grid_template_columns, b.style.grid_template_rows, b.style.grid_template_areas));
    }
    for c in &b.children {
        dump_layout(doc, c, depth + 1, out);
    }
}
