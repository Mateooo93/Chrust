p = 'src/style.rs'
s = open(p, encoding='utf-8').read()
s = s.replace('''#[derive(Clone, Debug)]
pub struct ComputedStyle {''', '''#[derive(Clone, Debug, PartialEq)]
pub struct Gradient {
    pub angle: f32,
    pub stops: Vec<(Color, f32)>,
}

#[derive(Clone, Debug)]
pub struct ComputedStyle {''')
s = s.replace('''    pub background_image: Option<String>,''', '''    pub background_image: Option<String>,
    pub background_gradient: Option<Gradient>,''')
s = s.replace('''            background_image: None,
            font_family:''', '''            background_image: None,
            background_gradient: None,
            font_family:''')
s = s.replace('''                for token in split_space_values_keep_functions(value) {
                    if token.starts_with("url(") {
                        image = Some(token);
                    } else if parse_color(&token).is_some()
                        || token.eq_ignore_ascii_case("transparent")
                    {
                        color = Some(token);
                    }
                }''', '''                for token in split_space_values_keep_functions(value) {
                    if token.starts_with("url(")
                        || token.contains("gradient(")
                    {
                        image = Some(token);
                    } else if parse_color(&token).is_some()
                        || token.eq_ignore_ascii_case("transparent")
                    {
                        color = Some(token);
                    }
                }''')
s = s.replace('''    if let Some(v) = get("background-image") {
        if v.trim().eq_ignore_ascii_case("none") {
            style.background_image = None;
        } else if let Some(url) = crate::css::extract_url(&v) {
            style.background_image = Some(url);
        }
    }''', '''    if let Some(v) = get("background-image") {
        let low = v.trim().to_ascii_lowercase();
        if low == "none" {
            style.background_image = None;
            style.background_gradient = None;
        } else if low.contains("gradient(") {
            if let Some(g) = parse_linear_gradient(&v) {
                style.background_gradient = Some(g);
            }
        } else if let Some(url) = crate::css::extract_url(&v) {
            style.background_image = Some(url);
        }
    }''')
s = s.replace('''pub fn parse_grid_areas(value: &str) -> Vec<Vec<String>> {''', '''pub fn parse_linear_gradient(value: &str) -> Option<Gradient> {
    let low = value.to_ascii_lowercase();
    let start = low.find("gradient(")?;
    let open = value[start..].find('(')? + start;
    let mut depth = 0i32;
    let mut end = None;
    for (i, c) in value[open..].char_indices() {
        match c {
            '(' => depth += 1,
            ')' => {
                depth -= 1;
                if depth == 0 {
                    end = Some(open + i);
                    break;
                }
            }
            _ => {}
        }
    }
    let end = end?;
    let inner = &value[open + 1..end];
    let parts = crate::css::split_top_level(inner, ',');
    if parts.len() < 2 {
        return None;
    }
    let mut angle = 180.0f32;
    let mut idx = 0;
    let first = parts[0].trim().to_ascii_lowercase();
    if first.starts_with("to ") || first.ends_with("deg") || first == "0" {
        let p = first.trim();
        angle = if p.ends_with("deg") {
            p.trim_end_matches("deg").trim().parse::<f32>().ok()?
        } else {
            match p {
                "to top" => 0.0,
                "to right" => 90.0,
                "to bottom" => 180.0,
                "to left" => 270.0,
                "to top right" | "to right top" => 45.0,
                "to bottom right" | "to right bottom" => 135.0,
                "to bottom left" | "to left bottom" => 225.0,
                "to top left" | "to left top" => 315.0,
                _ => 180.0,
            }
        };
        idx = 1;
    }
    let mut stops: Vec<(Color, Option<f32>)> = Vec::new();
    for part in parts[idx..].iter() {
        let mut color = None;
        let mut pos = None;
        for tok in part.split_whitespace() {
            if let Some(c) = parse_color_value(tok, Color::BLACK) {
                color = Some(c);
            } else if let Some(p) = tok.strip_suffix('%') {
                pos = p.parse::<f32>().ok().map(|v| (v / 100.0).clamp(0.0, 1.0));
            } else if let Ok(v) = tok.parse::<f32>() {
                let v = if v > 1.0 { v / 100.0 } else { v };
                pos = Some(v.clamp(0.0, 1.0));
            }
        }
        if let Some(c) = color {
            stops.push((c, pos));
        }
    }
    if stops.len() < 2 {
        return None;
    }
    let n = stops.len();
    let mut out: Vec<(Color, f32)> = Vec::with_capacity(n);
    for (i, (c, p)) in stops.iter().enumerate() {
        let p = p.unwrap_or(i as f32 / (n - 1) as f32);
        out.push((*c, p));
    }
    for i in 1..out.len() {
        if out[i].1 < out[i - 1].1 {
            out[i].1 = out[i - 1].1;
        }
    }
    Some(Gradient { angle, stops: out })
}

pub fn parse_grid_areas(value: &str) -> Vec<Vec<String>> {''')
open(p, 'w', encoding='utf-8').write(s)

p = 'src/paint.rs'
s = open(p, encoding='utf-8').read()
s = s.replace('''use crate::style::{BorderStyle, ComputedStyle, Display, Position, VerticalAlign};''',
              '''use crate::style::{BorderStyle, ComputedStyle, Display, Gradient, Position};''')
s = s.replace('''            if let Some(img) = &b.bg_image {
                if rect.intersects(&clip) {
                    let target = rect.intersect(&clip);
                    let _ = target;
                    self.draw_image(img, rect, clip);
                }
            }''', '''            if let Some(g) = &b.style.background_gradient {
                self.fill_gradient(rect, g, clip, opacity);
            }
            if let Some(img) = &b.bg_image {
                if rect.intersects(&clip) {
                    self.draw_image(img, rect, clip);
                }
            }''')
s = s.replace('''    fn paint_borders(&mut self, b: &LayoutBox, rect: Rect, clip: Rect, opacity: f32) {''', '''    fn fill_gradient(&mut self, rect: Rect, g: &Gradient, clip: Rect, opacity: f32) {
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

    fn paint_borders(&mut self, b: &LayoutBox, rect: Rect, clip: Rect, opacity: f32) {''')
s = s.replace('''fn scale_alpha(c: Color, opacity: f32) -> Color {''', '''fn gradient_color(g: &Gradient, t: f32) -> Color {
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

fn scale_alpha(c: Color, opacity: f32) -> Color {''')
open(p, 'w', encoding='utf-8').write(s)
print('gradients patched')
