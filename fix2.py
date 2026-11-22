import re

p = 'src/layout.rs'
s = open(p, encoding='utf-8').read()

start = s.index("    fn layout_flex(&mut self, b: &mut LayoutBox, cx: f32, cy: f32, inner_w: f32) -> f32 {")
end = s.index("    // ----------------------------------------------------------------- tables", start)
new_flex = r'''    fn layout_flex(&mut self, b: &mut LayoutBox, cx: f32, cy: f32, inner_w: f32) -> f32 {
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
        if is_row {
            let mut widths: Vec<f32> = Vec::with_capacity(n);
            for &i in &items {
                let child = &b.children[i];
                let basis = match child.style.flex_basis {
                    Length::Px(v) => v,
                    Length::Percent(p) => inner_w * p,
                    Length::Auto => {
                        if let Some(v) = crate::css::resolve_length(child.style.width, inner_w) {
                            v
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
            let mut heights: Vec<f32> = vec![0.0; n];
            let order: Vec<usize> = if reverse { (0..n).rev().collect() } else { (0..n).collect() };
            let mut placements: Vec<(usize, f32)> = Vec::new();
            for &k in &order {
                let i = items[k];
                let w = widths[k].max(0.0);
                {
                    let mut child = std::mem::replace(
                        &mut b.children[i],
                        LayoutBox::new(b.node, ComputedStyle::default(), BoxKind::Block),
                    );
                    child.style.width = Length::Px(w);
                    self.layout_block(&mut child, x, cy, w, false);
                    heights[k] = child.rect.h;
                    b.children[i] = child;
                }
                placements.push((k, x));
                x += w + gap + extra_gap;
            }
            let mut cross = 0.0f32;
            for h in &heights {
                cross = cross.max(*h);
            }
            if let Length::Px(h) = b.style.height {
                cross = h;
            } else if let Length::Percent(p) = b.style.height {
                cross = self.viewport.1 * p;
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
            }
            let total: f32 = heights.iter().sum::<f32>() + gap * n_gaps;
            let explicit_h = match b.style.height {
                Length::Px(v) => Some(v),
                Length::Percent(p) => Some(self.viewport.1 * p),
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

'''
s = s[:start] + new_flex + s[end:]

old = '''        if b.style.display == Display::Table {
            self.prepare_table(b, inner_w);
        }'''
new = '''        let mut inner_w = inner_w;
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
        }'''
s = s.replace(old, new)

start = s.index("    fn prepare_table(&mut self, b: &mut LayoutBox, inner_w: f32) {")
end = s.index("    // ---------------------------------------------------------------- markers", start)
new_tables = r'''    fn prepare_table(&mut self, b: &mut LayoutBox, inner_w: f32) {
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

'''
s = s[:start] + new_tables + s[end:]

s = s.replace('''        self.layout_block(&mut root, 0.0, 0.0, width, false);
        let page_height = root.rect.y + root.rect.h + root.margin.bottom;
        LayoutResult { root, page_height, page_width: width }''', '''        self.layout_block(&mut root, 0.0, 0.0, width, false);
        let mut content_bottom = root.rect.y + root.rect.h + root.margin.bottom;
        max_content_bottom(&root, &mut content_bottom, false);
        let page_height = content_bottom.max(self.viewport.1);
        LayoutResult { root, page_height, page_width: width }''')

s = s.replace('''fn kind_is_block(k: &BoxKind) -> bool {''', '''fn max_content_bottom(b: &LayoutBox, max_bottom: &mut f32, inside_fixed: bool) -> f32 {
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

fn kind_is_block(k: &BoxKind) -> bool {''')

s = s.replace('''trait LenExt {
    fn is_auto(&self) -> bool;
}
impl LenExt for Length {
    fn is_auto(&self) -> bool {
        matches!(self, Length::Auto)
    }
}

''', '')
open(p, 'w', encoding='utf-8').write(s)

p = 'src/style.rs'
s = open(p, encoding='utf-8').read()
s = s.replace('''        apply_winners(&mut style, &winners, &tag, viewport, parent_style.font_size, root_fs);
        apply_html_attributes(doc, node, &mut style, viewport);''', '''        apply_winners(&mut style, &winners, &tag, viewport, parent_style.font_size, root_fs);
        if (tag == "td" || tag == "th") && winners.get("text-align").is_none() {
            style.text_align = TextAlign::Left;
        }
        apply_html_attributes(doc, node, &mut style, viewport);''')
open(p, 'w', encoding='utf-8').write(s)
print('patched')
