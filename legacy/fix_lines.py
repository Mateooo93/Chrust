p = 'src/layout.rs'
s = open(p, encoding='utf-8').read()
start = s.index("    fn build_lines(")
end = s.index("    // ------------------------------------------------------------- replaced", start)
new = r'''    fn build_lines(
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
            f.x += dx;
            f.y = *y;
            f.height = lh;
            f.baseline = baseline;
            if f.atomic {
                let ay = match f.valign {
                    VerticalAlign::Top | VerticalAlign::TextTop => *y,
                    VerticalAlign::Middle => *y + (lh - f.height) / 2.0,
                    VerticalAlign::Bottom | VerticalAlign::TextBottom => *y + lh - f.height,
                    _ => baseline - f.height,
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

'''
s = s[:start] + new + s[end:]
open(p, 'w', encoding='utf-8').write(s)
print('lines rewritten')
