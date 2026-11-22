p = 'src/layout.rs'
s = open(p, encoding='utf-8').read()

# helper to compute horizontal extra (padding+border) for a box
s = s.replace('''    fn line_area(&self, y: f32, cx: f32, max_w: f32, indent: f32, line_h: f32) -> (f32, f32) {''', '''    fn horizontal_extra(&self, b: &LayoutBox, basis: f32) -> f32 {
        let pl = crate::css::resolve_length(b.style.padding[3], basis).unwrap_or(0.0);
        let pr = crate::css::resolve_length(b.style.padding[1], basis).unwrap_or(0.0);
        let bl = if b.style.border_style[3] != BorderStyle::None { b.style.border_width[3] } else { 0.0 };
        let br = if b.style.border_style[1] != BorderStyle::None { b.style.border_width[1] } else { 0.0 };
        pl + pr + bl + br
    }

    fn line_area(&self, y: f32, cx: f32, max_w: f32, indent: f32, line_h: f32) -> (f32, f32) {''')

# Row flex basis: make widths outer (border-box) and subtract extras when setting content width
s = s.replace('''                let basis = match child.style.flex_basis {
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
                widths.push(basis.max(0.0));''', '''                let extra = self.horizontal_extra(child, inner_w);
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
                widths.push(basis.max(0.0));''')

# When laying row items, set content width = outer - extra
s = s.replace('''                let w = widths[k].max(0.0);
                {
                    let mut child = std::mem::replace(
                        &mut b.children[i],
                        LayoutBox::new(b.node, ComputedStyle::default(), BoxKind::Block),
                    );
                    child.style.width = Length::Px(w);
                    self.layout_block(&mut child, x, cy, w, false);
                    heights[k] = child.rect.h;
                    b.children[i] = child;
                }''', '''                let outer_w = widths[k].max(0.0);
                let extra = self.horizontal_extra(&b.children[i], inner_w);
                let content_w = (outer_w - extra).max(1.0);
                {
                    let mut child = std::mem::replace(
                        &mut b.children[i],
                        LayoutBox::new(b.node, ComputedStyle::default(), BoxKind::Block),
                    );
                    child.style.width = Length::Px(content_w);
                    self.layout_block(&mut child, x, cy, content_w, false);
                    heights[k] = child.rect.h;
                    b.children[i] = child;
                }''')
s = s.replace('''                placements.push((k, x));
                x += w + gap + extra_gap;''', '''                placements.push((k, x));
                x += outer_w + gap + extra_gap;''')

# Column flex widths outer => subtract extras
s = s.replace('''            for &i in &items {
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
            }''', '''            for &i in &items {
                let extra = self.horizontal_extra(&b.children[i], inner_w);
                let outer_w = match b.children[i].style.width {
                    Length::Px(v) => v + extra,
                    Length::Percent(p) => inner_w * p,
                    Length::Auto => inner_w,
                };
                let content_w = (outer_w - extra).max(1.0);
                let mut cb = std::mem::replace(
                    &mut b.children[i],
                    LayoutBox::new(b.node, ComputedStyle::default(), BoxKind::Block),
                );
                cb.style.width = Length::Px(content_w);
                self.layout_block(&mut cb, cx, cy, content_w, false);
                heights.push(cb.rect.h);
                b.children[i] = cb;
            }''')

open(p, 'w', encoding='utf-8').write(s)
print('flexbox sizing patched')
