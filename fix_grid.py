p = 'src/style.rs'
s = open(p, encoding='utf-8').read()

s = s.replace('''    Flex,
    InlineFlex,
}''', '''    Flex,
    InlineFlex,
    Grid,
    InlineGrid,
}''')
s = s.replace('''                | Display::Flex
        )
    }
    pub fn is_inline_level(self) -> bool {
        matches!(self, Display::Inline | Display::InlineBlock | Display::InlineFlex)
    }''', '''                | Display::Flex
                | Display::Grid
        )
    }
    pub fn is_inline_level(self) -> bool {
        matches!(self, Display::Inline | Display::InlineBlock | Display::InlineFlex | Display::InlineGrid)
    }''')

s = s.replace('''    pub flex_basis: Length,
    pub gap: f32,
    pub row_gap: f32,''', '''    pub flex_basis: Length,
    pub gap: f32,
    pub row_gap: f32,
    pub grid_template_columns: String,
    pub grid_template_rows: String,
    pub grid_template_areas: Vec<Vec<String>>,
    pub grid_area: String,
    pub grid_column: String,
    pub grid_row: String,''')

s = s.replace('''            gap: 0.0,
            row_gap: 0.0,''', '''            gap: 0.0,
            row_gap: 0.0,
            grid_template_columns: String::new(),
            grid_template_rows: String::new(),
            grid_template_areas: Vec::new(),
            grid_area: String::new(),
            grid_column: String::new(),
            grid_row: String::new(),''')

s = s.replace('''            "flex" => Display::Flex,
            "inline-flex" => Display::InlineFlex,''', '''            "flex" => Display::Flex,
            "inline-flex" => Display::InlineFlex,
            "grid" => Display::Grid,
            "inline-grid" => Display::InlineGrid,''')

# grid property application before flex section
s = s.replace('''    // Flex.
    if let Some(v) = get("flex-direction") {''', '''    // Grid.
    if let Some(v) = get("grid-template-columns") {
        style.grid_template_columns = v.trim().to_string();
    }
    if let Some(v) = get("grid-template-rows") {
        style.grid_template_rows = v.trim().to_string();
    }
    if let Some(v) = get("grid-template-areas") {
        style.grid_template_areas = parse_grid_areas(&v);
    }
    if let Some(v) = get("grid-template") {
        let value = v.trim().to_string();
        let (rows_part, cols_part) = split_grid_template(&value);
        style.grid_template_areas = parse_grid_areas(&rows_part);
        let cleaned = strip_quoted_segments(&rows_part);
        if !cleaned.trim().is_empty() {
            style.grid_template_rows = cleaned.trim().to_string();
        }
        if !cols_part.trim().is_empty() {
            style.grid_template_columns = cols_part.trim().to_string();
        }
    }
    if let Some(v) = get("grid-area") {
        style.grid_area = v.trim().trim_matches('"').trim_matches('\\'').to_string();
    }
    if let Some(v) = get("grid-column") {
        style.grid_column = v.trim().to_string();
    } else if let Some(v) = get("grid-column-start") {
        style.grid_column = v.trim().to_string();
    }
    if let Some(v) = get("grid-row") {
        style.grid_row = v.trim().to_string();
    } else if let Some(v) = get("grid-row-start") {
        style.grid_row = v.trim().to_string();
    }
    // Flex.
    if let Some(v) = get("flex-direction") {''')

# helper functions
s = s.replace('''/// Best-effort text transform application used by layout.''', '''pub fn parse_grid_areas(value: &str) -> Vec<Vec<String>> {
    let mut rows: Vec<Vec<String>> = Vec::new();
    let chars: Vec<char> = value.chars().collect();
    let mut i = 0;
    while i < chars.len() {
        if chars[i] == '"' || chars[i] == '\\'' {
            let q = chars[i];
            i += 1;
            let start = i;
            while i < chars.len() && chars[i] != q {
                i += 1;
            }
            let content: String = chars[start..i].iter().collect();
            if i < chars.len() {
                i += 1;
            }
            let row: Vec<String> = content
                .split_whitespace()
                .map(|s| s.to_string())
                .collect();
            if !row.is_empty() {
                rows.push(row);
            }
        } else {
            i += 1;
        }
    }
    rows
}

pub fn split_grid_template(value: &str) -> (String, String) {
    let mut depth = 0i32;
    let mut quote: Option<char> = None;
    for (i, c) in value.char_indices() {
        if let Some(q) = quote {
            if c == q {
                quote = None;
            }
            continue;
        }
        match c {
            '"' | '\\'' => quote = Some(c),
            '(' => depth += 1,
            ')' => depth -= 1,
            '/' if depth == 0 => {
                return (value[..i].to_string(), value[i + 1..].to_string());
            }
            _ => {}
        }
    }
    (value.to_string(), String::new())
}

fn strip_quoted_segments(value: &str) -> String {
    let mut out = String::new();
    let mut quote: Option<char> = None;
    for c in value.chars() {
        if let Some(q) = quote {
            if c == q {
                quote = None;
            }
            continue;
        }
        match c {
            '"' | '\\'' => quote = Some(c),
            _ => out.push(c),
        }
    }
    out
}

/// Best-effort text transform application used by layout.''')
open(p, 'w', encoding='utf-8').write(s)

p = 'src/layout.rs'
s = open(p, encoding='utf-8').read()

# layout_children dispatch for grid
s = s.replace('''        let mut inner_w = inner_w;
        if b.style.display == Display::Table {''', '''        let mut inner_w = inner_w;
        if matches!(b.style.display, Display::Grid | Display::InlineGrid) {
            return self.layout_grid(b, cx, cy, inner_w);
        }
        if b.style.display == Display::Table {''')

# inline atomic condition includes InlineGrid
s = s.replace('''                if child.style.display == Display::InlineBlock
                    || child.style.display == Display::InlineFlex
                    || child.style.display == Display::Inline''', '''                if child.style.display == Display::InlineBlock
                    || child.style.display == Display::InlineFlex
                    || child.style.display == Display::InlineGrid
                    || child.style.display == Display::Inline''')

# intrinsic: grid row sum approx / grid block max
s = s.replace('''                if matches!(b.style.display, Display::Flex | Display::InlineFlex) {''', '''                if matches!(b.style.display, Display::Grid | Display::InlineGrid) {
                    let mut maxc: f32 = 0.0;
                    for c in &b.children {
                        maxc = maxc.max(self.intrinsic_width(c));
                    }
                    return maxc + extra;
                }
                if matches!(b.style.display, Display::Flex | Display::InlineFlex) {''')

# insert layout_grid before tables section
marker = "    // ----------------------------------------------------------------- tables"
grid_code = r'''    // ------------------------------------------------------------------ grid

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
        let row_tracks = parse_grid_tracks(&b.style.grid_template_rows, inner_w, 16.0);
        let area_cols = areas.iter().map(|r| r.len()).max().unwrap_or(0);
        let mut ncols = col_tracks.len().max(area_cols).max(1);
        let mut nrows = row_tracks.len().max(areas.len()).max(1);
        // Column/row placements.
        let mut placements: Vec<(usize, usize, usize, usize, usize)> = Vec::new(); // item index, row, col, rowspan, colspan
        let mut cursor_row = 0usize;
        let mut cursor_col = 0usize;
        for &i in &items {
            let child = &b.children[i];
            let mut placed = None;
            if !child.style.grid_area.is_empty() {
                if let Some((r0, c0, r1, c1)) = find_grid_area(&areas, &child.style.grid_area) {
                    placed = Some((r0, c0, (r1 - r0).max(1), (c1 - c0).max(1)));
                }
            }
            if placed.is_none() && !child.style.grid_column.is_empty() {
                let (start, span) = parse_grid_line(&child.style.grid_column);
                let (row_start, row_span) = parse_grid_line(&child.style.grid_row);
                placed = Some((row_start.unwrap_or(0), start.unwrap_or(0), row_span.unwrap_or(1), span.unwrap_or(1)));
            }
            let (r, c, rs, cs) = match placed {
                Some(p) => p,
                None => {
                    // First free cell.
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
                    (r, c, 1, 1)
                }
            };
            ncols = ncols.max(c + cs);
            nrows = nrows.max(r + rs);
            placements.push((i, r, c, rs, cs));
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
        let fixed: f32 = col_widths.iter().sum::<f32>() + col_gap * (ncols.saturating_sub(1)) as f32;
        let free = (inner_w - fixed).max(0.0);
        if fr_total > 0.0 {
            let unit = free / fr_total;
            for c in 0..ncols {
                if let Some(track) = col_tracks.get(c) {
                    if track.kind == TrackKind::Fr {
                        col_widths[c] += unit * track.fr;
                    }
                }
            }
        } else if !content_cols.is_empty() && free > 0.0 {
            let extra = free / content_cols.len() as f32;
            for c in &content_cols {
                col_widths[*c] += extra;
            }
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
                    child.style.width = Length::Px(w.max(1.0));
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
        // Position items.
        for (k, &(i, r, _c, _rs, _cs)) in placements.iter().enumerate() {
            let mut y = cy;
            for rr in 0..r {
                y += row_heights[rr] + row_gap;
            }
            let dy = y - b.children[i].rect.y;
            if dy != 0.0 {
                b.children[i].shift(0.0, dy);
            }
            let _ = k;
        }
        total_h.max(0.0)
    }

'''
s = s.replace(marker, grid_code + marker)

# track helpers near bottom before resolve_height_len
s = s.replace('''fn resolve_height_len(l: Length, basis: Option<f32>) -> Option<f32> {''', '''#[derive(Clone, Copy, Debug)]
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
                let count: usize = parts[0].trim().parse().unwrap_or(1).min(24);
                let inner_tracks = parse_grid_tracks(parts[1], inner_w, font_size);
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
                    });
                    continue;
                }
                if let Some(px) = parse_track_length(max_s, inner_w, font_size) {
                    out.push(GridTrack { kind: TrackKind::Px, base: px.max(min_px), fr: 0.0 });
                    continue;
                }
            }
            out.push(GridTrack { kind: TrackKind::Content, base: 0.0, fr: 0.0 });
            continue;
        }
        if t.ends_with("fr") {
            let fr: f32 = t.trim_end_matches("fr").trim().parse().unwrap_or(1.0);
            out.push(GridTrack { kind: TrackKind::Fr, base: 0.0, fr });
        } else if t == "auto" || t.contains("content") || t.starts_with("fit-content") {
            out.push(GridTrack { kind: TrackKind::Content, base: 0.0, fr: 0.0 });
        } else if let Some(px) = parse_track_length(t, inner_w, font_size) {
            out.push(GridTrack { kind: TrackKind::Px, base: px, fr: 0.0 });
        } else {
            out.push(GridTrack { kind: TrackKind::Content, base: 0.0, fr: 0.0 });
        }
    }
    out
}

fn parse_track_length(token: &str, inner_w: f32, font_size: f32) -> Option<f32> {
    let t = token.trim();
    if t == "0" {
        return Some(0.0);
    }
    let len = crate::css::parse_length(t, font_size, 16.0, (inner_w, 800.0))?;
    crate::css::resolve_length(len, inner_w)
}

fn split_track_tokens(value: &str) -> Vec<String> {
    let mut out = Vec::new();
    let mut cur = String::new();
    let mut depth = 0i32;
    for c in value.chars() {
        match c {
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

fn parse_grid_line(value: &str) -> (Option<usize>, Option<usize>) {
    let v = value.trim();
    if v.is_empty() || v == "auto" {
        return (None, None);
    }
    if let Some(span) = v.strip_prefix("span ") {
        return (None, span.trim().parse::<usize>().ok());
    }
    let parts: Vec<&str> = v.split('/').map(|p| p.trim()).collect();
    if parts.len() == 2 {
        let start = parts[0].parse::<usize>().ok().map(|n| n.saturating_sub(1));
        let end = parts[1].parse::<usize>().ok();
        let span = match (start, end) {
            (Some(s), Some(e)) if e > s => Some(e - s),
            _ => None,
        };
        return (start, span);
    }
    if let Ok(n) = v.parse::<usize>() {
        return (Some(n.saturating_sub(1)), None);
    }
    (None, None)
}

fn resolve_height_len(l: Length, basis: Option<f32>) -> Option<f32> {''')
open(p, 'w', encoding='utf-8').write(s)
print('grid patched')
