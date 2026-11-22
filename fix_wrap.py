p = 'src/layout.rs'
s = open(p, encoding='utf-8').read()
s = s.replace('''        if is_row {
            let mut widths: Vec<f32> = Vec::with_capacity(n);''', '''        if is_row && b.style.flex_wrap {
            return self.layout_flex_wrap_row(b, cx, cy, inner_w, &items);
        }
        if is_row {
            let mut widths: Vec<f32> = Vec::with_capacity(n);''')

marker = '''    // ----------------------------------------------------------------- tables'''
helper = r'''    fn layout_flex_wrap_row(
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

'''
s = s.replace(marker, helper + marker)
open(p, 'w', encoding='utf-8').write(s)
print('flex wrap patched')
