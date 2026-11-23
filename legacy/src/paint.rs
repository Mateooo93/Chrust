#![allow(dead_code)]
//! Software rasterizer: paints the layout tree into a 32-bit pixmap.

use crate::dom::Document;
use crate::fonts::FontStore;
use crate::layout::{ImageData, LayoutBox, LayoutResult, Rect};
use crate::style::{BorderStyle, ComputedStyle, Display, Gradient, Position};
use crate::util::Color;
use std::collections::HashMap;

pub struct Pixmap {
    pub width: usize,
    pub height: usize,
    pub data: Vec<u32>, // 0x00RRGGBB
}

impl Pixmap {
    pub fn new(width: usize, height: usize, fill: Color) -> Pixmap {
        Pixmap { width, height, data: vec![fill.to_u32(); width * height] }
    }

    pub fn resize(&mut self, width: usize, height: usize, fill: Color) {
        self.width = width;
        self.height = height;
        self.data = vec![fill.to_u32(); width * height];
    }

    pub fn clear(&mut self, color: Color) {
        self.data.fill(color.to_u32());
    }

    #[inline]
    pub fn blend_pixel(&mut self, x: i32, y: i32, color: Color, alpha: u8) {
        if x < 0 || y < 0 || x >= self.width as i32 || y >= self.height as i32 || alpha == 0 {
            return;
        }
        let idx = y as usize * self.width + x as usize;
        if alpha == 255 && color.a == 255 {
            self.data[idx] = color.to_u32();
            return;
        }
        let a = (alpha as u32) * (color.a as u32) / 255;
        if a == 0 {
            return;
        }
        let dst = self.data[idx];
        let dr = ((dst >> 16) & 0xFF) as u32;
        let dg = ((dst >> 8) & 0xFF) as u32;
        let db = (dst & 0xFF) as u32;
        let sr = color.r as u32;
        let sg = color.g as u32;
        let sb = color.b as u32;
        let inv = 255 - a;
        let r = (sr * a + dr * inv) / 255;
        let g = (sg * a + dg * inv) / 255;
        let b = (sb * a + db * inv) / 255;
        self.data[idx] = (r << 16) | (g << 8) | b;
    }

    pub fn blend_pixel_color(&mut self, x: i32, y: i32, color: Color) {
        self.blend_pixel(x, y, color, 255);
    }

    pub fn fill_rect(&mut self, rect: Rect, color: Color) {
        if color.a == 0 || rect.w <= 0.0 || rect.h <= 0.0 {
            return;
        }
        let x0 = rect.x.floor().max(0.0) as i32;
        let y0 = rect.y.floor().max(0.0) as i32;
        let x1 = (rect.x + rect.w).ceil().min(self.width as f32) as i32;
        let y1 = (rect.y + rect.h).ceil().min(self.height as f32) as i32;
        for y in y0..y1 {
            for x in x0..x1 {
                self.blend_pixel(x, y, color, 255);
            }
        }
    }

    pub fn fill_rect_clipped(&mut self, rect: Rect, clip: Rect, color: Color) {
        let r = rect.intersect(&clip);
        self.fill_rect(r, color);
    }

    pub fn stroke_rect(&mut self, rect: Rect, width: f32, color: Color, clip: Rect) {
        if width <= 0.0 || color.a == 0 {
            return;
        }
        let w = width.max(0.5);
        self.fill_rect_clipped(Rect::new(rect.x, rect.y, rect.w, w), clip, color);
        self.fill_rect_clipped(Rect::new(rect.x, rect.y + rect.h - w, rect.w, w), clip, color);
        self.fill_rect_clipped(Rect::new(rect.x, rect.y, w, rect.h), clip, color);
        self.fill_rect_clipped(Rect::new(rect.x + rect.w - w, rect.y, w, rect.h), clip, color);
    }

    pub fn fill_rounded_rect(&mut self, rect: Rect, radius: f32, color: Color, clip: Rect) {
        if radius <= 0.5 {
            self.fill_rect_clipped(rect, clip, color);
            return;
        }
        let r = rect.intersect(&clip);
        if r.w <= 0.0 || r.h <= 0.0 {
            return;
        }
        let rad = radius.min(rect.w / 2.0).min(rect.h / 2.0);
        let x0 = r.x.floor() as i32;
        let y0 = r.y.floor() as i32;
        let x1 = (r.x + r.w).ceil() as i32;
        let y1 = (r.y + r.h).ceil() as i32;
        for y in y0..y1 {
            for x in x0..x1 {
                let px = x as f32 + 0.5;
                let py = y as f32 + 0.5;
                let cx = if px < rect.x + rad {
                    rect.x + rad
                } else if px > rect.x + rect.w - rad {
                    rect.x + rect.w - rad
                } else {
                    px
                };
                let cy = if py < rect.y + rad {
                    rect.y + rad
                } else if py > rect.y + rect.h - rad {
                    rect.y + rect.h - rad
                } else {
                    py
                };
                let dx = px - cx;
                let dy = py - cy;
                if dx * dx + dy * dy <= rad * rad {
                    self.blend_pixel(x, y, color, 255);
                }
            }
        }
    }
}

pub struct Painter<'a> {
    pub pix: &'a mut Pixmap,
    pub fonts: &'a mut FontStore,
    pub doc: &'a Document,
    pub scroll: f32,
    pub origin_y: f32,
    pub clip: Rect,
    pub viewport: Rect,
    pub images: &'a HashMap<String, ImageData>,
}

impl<'a> Painter<'a> {
    pub fn new(
        pix: &'a mut Pixmap,
        fonts: &'a mut FontStore,
        doc: &'a Document,
        images: &'a HashMap<String, ImageData>,
        scroll: f32,
        origin_y: f32,
    ) -> Painter<'a> {
        let clip = Rect::new(0.0, origin_y, pix.width as f32, pix.height as f32 - origin_y);
        let viewport = Rect::new(0.0, origin_y, pix.width as f32, pix.height as f32 - origin_y);
        Painter { pix, fonts, doc, scroll, origin_y, clip, viewport, images }
    }

    pub fn paint_page(&mut self, layout: &LayoutResult) {
        // Page background.
        let root_bg = layout.root.style.background_color;
        let bg = if root_bg.a > 0 { root_bg } else { Color::WHITE };
        self.pix.fill_rect(self.clip, bg);
        if let Some(img) = &layout.root.bg_image {
            self.draw_image(img, self.clip, self.clip);
        }
        let clip = self.clip;
        self.paint_box(&layout.root, false, clip);
    }

    fn paint_box(&mut self, b: &LayoutBox, fixed_ctx: bool, clip: Rect) {
        if b.style.display == Display::None || b.style.opacity <= 0.0 {
            return;
        }
        let fixed = fixed_ctx || b.style.position == Position::Fixed;
        let scroll = if fixed { 0.0 } else { self.scroll };
        let oy = self.origin_y - scroll;
        let rect = Rect::new(b.rect.x, b.rect.y + oy, b.rect.w, b.rect.h);
        let clip = if b.style.overflow_hidden && rect.w > 0.0 && rect.h > 0.0 {
            rect.intersect(&clip)
        } else {
            clip
        };
        let replaced = matches!(
            b.kind,
            crate::layout::BoxKind::Input { .. }
                | crate::layout::BoxKind::Textarea { .. }
                | crate::layout::BoxKind::Select { .. }
                | crate::layout::BoxKind::Image
        );
        let is_inline_container = b.style.display == Display::Inline && !replaced;
        let visible = b.style.visible;
        let opacity = b.style.opacity.clamp(0.0, 1.0);
        if visible && !is_inline_container && rect.w > 0.0 && rect.h > 0.0 && rect.intersects(&clip) {
            // Background.
            if b.style.background_color.a > 0 && b.style.mask_image.is_none() {
                let color = scale_alpha(b.style.background_color, opacity);
                if b.style.border_radius > 0.5 {
                    self.pix.fill_rounded_rect(rect, b.style.border_radius, color, clip);
                } else {
                    self.pix.fill_rect_clipped(rect, clip, color);
                }
            }
            if let Some(g) = &b.style.background_gradient {
                self.fill_gradient(rect, g, clip, opacity);
            }
            if let Some(img) = &b.bg_image {
                if rect.intersects(&clip) {
                    let style = b.style.clone();
                    self.draw_background(img, rect, &style, clip);
                }
            } else if let Some(img) = &b.mask_image {
                self.draw_image(img, rect, clip);
            }
            // Borders.
            self.paint_borders(b, rect, clip, opacity);
        }

        // List marker.
        if visible {
            if let Some(m) = &b.marker {
                self.paint_fragment(m, oy, clip, fixed);
            }
        }

        // Inline content.
        if visible {
            for line in &b.lines {
                for f in &line.fragments {
                    if f.atomic {
                        continue;
                    }
                    self.paint_fragment(f, oy, clip, fixed);
                }
            }
        }

        // Children (block boxes, inline-block atoms, inline descendants).
        // Positioned boxes paint above in-flow content, ordered by z-index.
        let mut in_flow: Vec<&LayoutBox> = Vec::new();
        let mut positioned: Vec<&LayoutBox> = Vec::new();
        for child in &b.children {
            if child.style.display == Display::None {
                continue;
            }
            if matches!(child.style.position, Position::Absolute | Position::Fixed) {
                positioned.push(child);
            } else {
                in_flow.push(child);
            }
        }
        positioned.sort_by_key(|c| c.style.z_index);
        for child in in_flow.into_iter().chain(positioned.into_iter()) {
            match child.kind {
                crate::layout::BoxKind::Text(_) => continue,
                crate::layout::BoxKind::Break => continue,
                crate::layout::BoxKind::Inline => {
                    if child.style.display == Display::Inline {
                        // Recurse to paint nested replaced/inline-block boxes.
                        for gc in &child.children {
                            if !matches!(
                                gc.kind,
                                crate::layout::BoxKind::Text(_) | crate::layout::BoxKind::Break
                            ) {
                                self.paint_box(gc, fixed, clip);
                            }
                        }
                        continue;
                    }
                    self.paint_box(child, fixed, clip);
                }
                _ => {
                    self.paint_box(child, fixed, clip);
                }
            }
        }

        // Form controls draw their own content.
        if visible {
            match &b.kind {
                crate::layout::BoxKind::Input { input_type, value, placeholder, checked } => {
                    self.paint_input(b, input_type, value, placeholder, *checked, rect, oy, clip, opacity);
                }
                crate::layout::BoxKind::Textarea { value, placeholder } => {
                    self.paint_textarea(b, value, placeholder, rect, clip, opacity);
                }
                crate::layout::BoxKind::Select { value } => {
                    self.paint_select(b, value, rect, clip, opacity);
                }
                crate::layout::BoxKind::Image => {
                    let target = b.content_rect;
                    let target = Rect::new(target.x, target.y + oy, target.w, target.h);
                    if let Some(img) = &b.image {
                        self.draw_image(img, target, clip);
                    } else {
                        self.pix.fill_rect_clipped(target, clip, Color::rgb(240, 240, 240));
                        self.pix.stroke_rect(target, 1.0, Color::rgb(200, 200, 200), clip);
                        let alt = self.doc.attr(b.node, "alt").unwrap_or("").to_string();
                        if !alt.is_empty() {
                            let style = b.style.clone();
                            let (asc, _) = self.fonts.line_metrics(&style);
                            let baseline = target.y + (target.h / 2.0) + asc / 2.0;
                            self.draw_text(&alt, &style, target.x + 4.0, baseline, Color::rgb(120, 120, 120), clip);
                        }
                    }
                }
                _ => {}
            }
        }
    }

    fn fill_gradient(&mut self, rect: Rect, g: &Gradient, clip: Rect, opacity: f32) {
        let r = rect.intersect(&clip);
        if r.w <= 0.0 || r.h <= 0.0 {
            return;
        }
        let theta = g.angle.to_radians();
        let dx = theta.sin();
        let dy = -theta.cos();
        // Project corners to normalize the gradient across the box.
        let corners = [
            (rect.x, rect.y),
            (rect.x + rect.w, rect.y),
            (rect.x, rect.y + rect.h),
            (rect.x + rect.w, rect.y + rect.h),
        ];
        let mut minp = f32::MAX;
        let mut maxp = f32::MIN;
        for (x, y) in corners {
            let p = x * dx + y * dy;
            minp = minp.min(p);
            maxp = maxp.max(p);
        }
        let span = (maxp - minp).max(1.0);
        let x0 = r.x.floor() as i32;
        let y0 = r.y.floor() as i32;
        let x1 = r.right().ceil() as i32;
        let y1 = r.bottom().ceil() as i32;
        for y in y0..y1 {
            for x in x0..x1 {
                let t = (((x as f32 + 0.5) * dx + (y as f32 + 0.5) * dy) - minp) / span;
                let c = gradient_color(g, t.clamp(0.0, 1.0));
                self.pix.blend_pixel(x, y, scale_alpha(c, opacity), 255);
            }
        }
    }

    fn paint_borders(&mut self, b: &LayoutBox, rect: Rect, clip: Rect, opacity: f32) {
        let sides = [
            (0usize, Rect::new(rect.x, rect.y, rect.w, b.border.top)),
            (1, Rect::new(rect.x + rect.w - b.border.right, rect.y, b.border.right, rect.h)),
            (2, Rect::new(rect.x, rect.y + rect.h - b.border.bottom, rect.w, b.border.bottom)),
            (3, Rect::new(rect.x, rect.y, b.border.left, rect.h)),
        ];
        for (i, r) in sides {
            if b.style.border_width[i] <= 0.0 || b.style.border_style[i] == BorderStyle::None {
                continue;
            }
            let color = scale_alpha(b.style.border_color[i], opacity);
            match b.style.border_style[i] {
                BorderStyle::Dashed | BorderStyle::Dotted => {
                    let horizontal = i == 0 || i == 2;
                    let len = if horizontal { r.w } else { r.h };
                    let mut pos = 0.0;
                    let dash: f32 = if b.style.border_style[i] == BorderStyle::Dotted { 2.0 } else { 5.0 };
                    while pos < len {
                        let seg = dash.min(len - pos);
                        let seg_rect = if horizontal {
                            Rect::new(r.x + pos, r.y, seg, r.h)
                        } else {
                            Rect::new(r.x, r.y + pos, r.w, seg)
                        };
                        self.pix.fill_rect_clipped(seg_rect, clip, color);
                        pos += dash * 2.0;
                    }
                }
                _ => {
                    self.pix.fill_rect_clipped(r, clip, color);
                }
            }
        }
    }

    fn paint_fragment(&mut self, f: &crate::layout::LineFragment, oy: f32, clip: Rect, fixed: bool) {
        if !f.style.visible {
            return;
        }
        let y = f.y + if fixed { self.origin_y } else { oy };
        let _ = fixed;
        let rect = Rect::new(f.x, y, f.width, f.height);
        // Inline background.
        if f.style.background_color.a > 0 && !f.is_space && f.width > 0.0 {
            let bg = scale_alpha(f.style.background_color, f.style.opacity);
            self.pix.fill_rect_clipped(rect, clip, bg);
        }
        if f.atomic || f.text.is_empty() {
            return;
        }
        let color = scale_alpha(f.style.color, f.style.opacity);
        self.draw_text(&f.text, &f.style, f.x, f.baseline + (y - f.y), color, clip);
        if f.style.text_decoration.underline {
            let uy = f.baseline + (y - f.y) + f.style.font_size * 0.08 + 1.0;
            self.pix.fill_rect_clipped(
                Rect::new(f.x, uy, f.width, 1.0),
                clip,
                color,
            );
        }
        if f.style.text_decoration.line_through {
            let uy = f.baseline + (y - f.y) - f.style.font_size * 0.3;
            self.pix.fill_rect_clipped(Rect::new(f.x, uy, f.width, 1.0), clip, color);
        }
        if f.style.text_decoration.overline {
            let uy = f.baseline + (y - f.y) - f.style.font_size * 0.8;
            self.pix.fill_rect_clipped(Rect::new(f.x, uy, f.width, 1.0), clip, color);
        }
    }

    pub fn draw_text(&mut self, text: &str, style: &ComputedStyle, x: f32, baseline: f32, color: Color, clip: Rect) {
        draw_text_run(self.pix, self.fonts, text, style, x, baseline, color, clip);
    }

    fn draw_background(
        &mut self,
        img: &ImageData,
        area: Rect,
        style: &ComputedStyle,
        clip: Rect,
    ) {
        if area.w <= 0.0 || area.h <= 0.0 || img.width == 0 || img.height == 0 {
            return;
        }
        let iw = img.width as f32;
        let ih = img.height as f32;
        let scale = match style.bg_size.as_str() {
            "cover" => (area.w / iw).max(area.h / ih),
            "contain" => (area.w / iw).min(area.h / ih),
            _ => 1.0,
        };
        let (mut dw, mut dh) = (iw * scale, ih * scale);
        if let Some((sw, sh)) = style.bg_size_px {
            if sw > 0.0 {
                dw = sw;
                dh = if sh > 0.0 { sh } else { ih * (sw / iw) };
            } else if sh > 0.0 {
                dh = sh;
                dw = iw * (sh / ih);
            }
        }
        let dw = dw.max(1.0);
        let dh = dh.max(1.0);
        // Backgrounds are clipped to the element's border box.
        let clip = clip.intersect(&area);
        if style.bg_no_repeat {
            let x = match style.bg_pos_px_x {
                Some(v) => area.x + v,
                None => area.x + (area.w - dw) * style.bg_pos_x,
            };
            let y = match style.bg_pos_px_y {
                Some(v) => area.y + v,
                None => area.y + (area.h - dh) * style.bg_pos_y,
            };
            self.draw_image(img, Rect::new(x, y, dw, dh), clip);
            return;
        }
        // Tiled background inside the padding box.
        let start_x = match style.bg_pos_px_x {
            Some(v) => area.x + v,
            None => area.x - (area.w - dw) * style.bg_pos_x,
        };
        let start_y = match style.bg_pos_px_y {
            Some(v) => area.y + v,
            None => area.y - (area.h - dh) * style.bg_pos_y,
        };
        let limit = area.intersect(&clip);
        let mut count = 0;
        let mut y = start_y;
        while y < limit.bottom() && count < 400 {
            let mut x = start_x;
            while x < limit.right() && count < 400 {
                if x + dw > limit.x && y + dh > limit.y {
                    self.draw_image(img, Rect::new(x, y, dw, dh), clip);
                    count += 1;
                }
                x += dw;
            }
            y += dh;
        }
    }

    fn draw_image(&mut self, img: &ImageData, target: Rect, clip: Rect) {
        let dest = target.intersect(&clip);
        if dest.w <= 0.0 || dest.h <= 0.0 || img.width == 0 || img.height == 0 {
            return;
        }
        let dw = img.width as f32;
        let dh = img.height as f32;
        let x0 = dest.x.floor() as i32;
        let y0 = dest.y.floor() as i32;
        let x1 = dest.right().ceil() as i32;
        let y1 = dest.bottom().ceil() as i32;
        for y in y0..y1 {
            let ty = (y as f32 + 0.5 - target.y) / target.h;
            if !(0.0..1.0).contains(&ty) {
                continue;
            }
            let sy = (ty * dh).floor().min(dh - 1.0) as u32;
            for x in x0..x1 {
                let tx = (x as f32 + 0.5 - target.x) / target.w;
                if !(0.0..1.0).contains(&tx) {
                    continue;
                }
                let sx = (tx * dw).floor().min(dw - 1.0) as u32;
                let idx = ((sy * img.width + sx) * 4) as usize;
                if idx + 3 < img.pixels.len() {
                    let a = img.pixels[idx + 3];
                    if a > 0 {
                        let c = Color::rgba(img.pixels[idx], img.pixels[idx + 1], img.pixels[idx + 2], 255);
                        self.pix.blend_pixel(x, y, c, a);
                    }
                }
            }
        }
    }

    fn paint_input(
        &mut self,
        b: &LayoutBox,
        input_type: &str,
        value: &str,
        placeholder: &str,
        checked: bool,
        rect: Rect,
        oy: f32,
        clip: Rect,
        opacity: f32,
    ) {
        let content = Rect::new(
            b.content_rect.x,
            b.content_rect.y + oy,
            b.content_rect.w,
            b.content_rect.h,
        );
        let (asc, desc) = self.fonts.line_metrics(&b.style);
        let text_color = scale_alpha(b.style.color, opacity);
        match input_type {
            "checkbox" | "radio" => {
                if input_type == "radio" {
                    let cx = rect.x + rect.w / 2.0;
                    let cy = rect.y + rect.h / 2.0;
                    let r = rect.w.min(rect.h) / 2.0 - 1.0;
                    for y in 0..rect.h as i32 {
                        for x in 0..rect.w as i32 {
                            let dx = x as f32 + 0.5 - rect.w / 2.0;
                            let dy = y as f32 + 0.5 - rect.h / 2.0;
                            if dx * dx + dy * dy <= r * r {
                                self.pix.blend_pixel(rect.x as i32 + x, rect.y as i32 + y, Color::WHITE, 255);
                            }
                        }
                    }
                    let _ = (cx, cy);
                    if checked {
                        for y in 0..rect.h as i32 {
                            for x in 0..rect.w as i32 {
                                let dx = x as f32 + 0.5 - rect.w / 2.0;
                                let dy = y as f32 + 0.5 - rect.h / 2.0;
                                if dx * dx + dy * dy <= (r - 2.5).max(1.0).powi(2) {
                                    self.pix.blend_pixel(rect.x as i32 + x, rect.y as i32 + y, Color::rgb(40, 80, 200), 255);
                                }
                            }
                        }
                    }
                } else {
                    self.pix.fill_rect_clipped(rect, clip, Color::WHITE);
                    self.pix.stroke_rect(rect, 1.0, Color::rgb(118, 118, 118), clip);
                    if checked {
                        let mut x = rect.x + 2.5;
                        let mut y = rect.y + rect.h / 2.0;
                        for i in 0..5 {
                            let px = (x + i as f32 * 1.4).round() as i32;
                            let py = (y - i as f32 * 0.5).round() as i32;
                            self.pix.blend_pixel(px, py, Color::rgb(30, 60, 160), 255);
                            self.pix.blend_pixel(px, py + 1, Color::rgb(30, 60, 160), 255);
                        }
                        x += 7.0;
                        for i in 0..7 {
                            let px = (x + i as f32 * 1.4).round() as i32;
                            let py = (y + i as f32 * 1.1).round() as i32;
                            self.pix.blend_pixel(px, py, Color::rgb(30, 60, 160), 255);
                            self.pix.blend_pixel(px + 1, py, Color::rgb(30, 60, 160), 255);
                        }
                    }
                }
            }
            "submit" | "button" | "reset" => {
                let label = if value.is_empty() {
                    if input_type == "submit" {
                        "Submit".to_string()
                    } else if input_type == "reset" {
                        "Reset".to_string()
                    } else {
                        "Button".to_string()
                    }
                } else {
                    value.to_string()
                };
                let w = self.fonts.measure_style(&label, &b.style);
                let tx = rect.x + (rect.w - w) / 2.0;
                let baseline = rect.y + rect.h / 2.0 + (asc - desc) / 2.0 - 1.0;
                self.draw_text(&label, &b.style, tx, baseline, text_color, clip);
            }
            "hidden" => {}
            _ => {
                let shown = if input_type == "password" {
                    "•".repeat(value.chars().count())
                } else {
                    value.to_string()
                };
                let (text, color) = if shown.is_empty() && !placeholder.is_empty() {
                    (placeholder.to_string(), Color::rgb(117, 117, 117))
                } else {
                    (shown, text_color)
                };
                let baseline = content.y + content.h / 2.0 + (asc - desc) / 2.0 - 1.0;
                let old_clip = self.clip;
                let inner = Rect::new(content.x + 1.0, content.y, (content.w - 4.0).max(0.0), content.h);
                let clipped = inner.intersect(&clip);
                self.clip = clipped;
                self.draw_text(&text, &b.style, content.x + 2.0, baseline, color, clipped);
                self.clip = old_clip;
            }
        }
    }

    fn paint_textarea(&mut self, b: &LayoutBox, value: &str, _placeholder: &str, rect: Rect, clip: Rect, opacity: f32) {
        let _ = rect;
        let content = b.content_rect;
        let oy = self.origin_y - (if b.style.position == Position::Fixed { 0.0 } else { self.scroll });
        let content = Rect::new(content.x, content.y + oy, content.w, content.h);
        let (asc, _desc) = self.fonts.line_metrics(&b.style);
        let color = scale_alpha(b.style.color, opacity);
        let mut y = content.y + asc;
        for line in value.split('\n').take(50) {
            if y > content.bottom() {
                break;
            }
            self.draw_text(line, &b.style, content.x + 2.0, y, color, clip);
            y += b.style.line_height;
        }
    }

    fn paint_select(&mut self, b: &LayoutBox, value: &str, rect: Rect, clip: Rect, opacity: f32) {
        let content = b.content_rect;
        let oy = self.origin_y - (if b.style.position == Position::Fixed { 0.0 } else { self.scroll });
        let content = Rect::new(content.x, content.y + oy, content.w, content.h);
        let (asc, desc) = self.fonts.line_metrics(&b.style);
        let color = scale_alpha(b.style.color, opacity);
        let baseline = content.y + content.h / 2.0 + (asc - desc) / 2.0 - 1.0;
        self.draw_text(value, &b.style, content.x + 2.0, baseline, color, clip);
        // Arrow.
        let ax = rect.right() - 12.0;
        let ay = rect.y + rect.h / 2.0;
        for i in 0..4 {
            self.pix.fill_rect_clipped(
                Rect::new(ax + i as f32, ay - 2.0 + i as f32, 8.0 - 2.0 * i as f32, 1.0),
                clip,
                Color::rgb(60, 60, 60),
            );
        }
    }
}

fn gradient_color(g: &Gradient, t: f32) -> Color {
    if g.stops.is_empty() {
        return Color::BLACK;
    }
    if t <= g.stops[0].1 {
        return g.stops[0].0;
    }
    for w in g.stops.windows(2) {
        let (c0, p0) = w[0];
        let (c1, p1) = w[1];
        if t >= p0 && t <= p1 {
            let f = if (p1 - p0).abs() < 1e-6 { 0.0 } else { (t - p0) / (p1 - p0) };
            let lerp = |a: u8, b: u8| ((a as f32) * (1.0 - f) + (b as f32) * f).round() as u8;
            return Color::rgba(lerp(c0.r, c1.r), lerp(c0.g, c1.g), lerp(c0.b, c1.b), lerp(c0.a, c1.a));
        }
    }
    g.stops.last().unwrap().0
}

fn scale_alpha(c: Color, opacity: f32) -> Color {
    let a = (c.a as f32 * opacity.clamp(0.0, 1.0)).round() as u8;
    Color { a, ..c }
}

/// Draw a text run with per-glyph fallback and antialiasing. Shared by page
/// painting and browser chrome.
pub fn draw_text_run(
    pix: &mut Pixmap,
    fonts: &mut FontStore,
    text: &str,
    style: &ComputedStyle,
    x: f32,
    baseline: f32,
    color: Color,
    clip: Rect,
) {
    let mut pen = x;
    for ch in text.chars() {
        if ch == '\n' || ch == '\r' {
            continue;
        }
        if ch == ' ' || ch == '\u{00A0}' {
            let s = ch.to_string();
            pen += fonts.measure_style(&s, style);
            continue;
        }
        let glyph = fonts.glyph(ch, style);
        if glyph.width > 0 && glyph.height > 0 {
            let gx = (pen + glyph.left).round() as i32;
            let gy = (baseline - glyph.top).round() as i32;
            let gw = glyph.width as i32;
            let gh = glyph.height as i32;
            if (gx as f32) < clip.right()
                && (gx + gw) as f32 > clip.x
                && (gy as f32) < clip.bottom()
                && (gy + gh) as f32 > clip.y
            {
                for row in 0..gh {
                    let py = gy + row;
                    if (py as f32) < clip.y || (py as f32) >= clip.bottom() {
                        continue;
                    }
                    for col in 0..gw {
                        let a = glyph.bitmap[(row * gw + col) as usize];
                        if a > 0 {
                            let px = gx + col;
                            if (px as f32) >= clip.x && (px as f32) < clip.right() {
                                pix.blend_pixel(px, py, color, a);
                            }
                        }
                    }
                }
            }
        }
        pen += glyph.advance + style.letter_spacing;
        if ch == ' ' {
            pen += style.word_spacing;
        }
    }
}

pub fn render_page(
    pix: &mut Pixmap,
    layout: &LayoutResult,
    doc: &Document,
    fonts: &mut FontStore,
    images: &HashMap<String, ImageData>,
    scroll: f32,
    origin_y: f32,
) {
    let mut painter = Painter::new(pix, fonts, doc, images, scroll, origin_y);
    painter.paint_page(layout);
}

/// Save a pixmap as a PNG.
pub fn save_png(pix: &Pixmap, path: &str) -> Result<(), String> {
    let mut buf = Vec::with_capacity(pix.width * pix.height * 4);
    for &px in &pix.data {
        buf.push(((px >> 16) & 0xFF) as u8);
        buf.push(((px >> 8) & 0xFF) as u8);
        buf.push((px & 0xFF) as u8);
        buf.push(255);
    }
    let img = image::RgbaImage::from_raw(pix.width as u32, pix.height as u32, buf)
        .ok_or_else(|| "Bad image buffer".to_string())?;
    img.save(path).map_err(|e| e.to_string())
}
