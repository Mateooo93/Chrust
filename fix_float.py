p = 'src/style.rs'
s = open(p, encoding='utf-8').read()
s = s.replace('''#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum VerticalAlign {''', '''#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Float {
    None,
    Left,
    Right,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum VerticalAlign {''')
s = s.replace('''    pub opacity: f32,
    pub overflow_hidden: bool,''', '''    pub opacity: f32,
    pub float: Float,
    pub overflow_hidden: bool,''')
s = s.replace('''            opacity: 1.0,
            overflow_hidden: false,''', '''            opacity: 1.0,
            float: Float::None,
            overflow_hidden: false,''')
s = s.replace('''    if let Some(v) = get("overflow") {
        let low = v.trim().to_ascii_lowercase();
        style.overflow_hidden = matches!(low.as_str(), "hidden" | "auto" | "scroll" | "clip");
    }''', '''    if let Some(v) = get("overflow") {
        let low = v.trim().to_ascii_lowercase();
        style.overflow_hidden = matches!(low.as_str(), "hidden" | "auto" | "scroll" | "clip");
    }
    if let Some(v) = get("float") {
        style.float = match v.trim().to_ascii_lowercase().as_str() {
            "left" => Float::Left,
            "right" => Float::Right,
            _ => Float::None,
        };
    }''')
open(p, 'w', encoding='utf-8').write(s)

p = 'src/layout.rs'
s = open(p, encoding='utf-8').read()
s = s.replace('''use crate::style::{
    transform_text, AlignItems, BorderStyle, ComputedStyle, Display, FlexDirection, JustifyContent,
    ListStyleType, Position, TextAlign, VerticalAlign, WhiteSpace,
};''', '''use crate::style::{
    transform_text, AlignItems, BorderStyle, ComputedStyle, Display, FlexDirection, Float,
    JustifyContent, ListStyleType, Position, TextAlign, VerticalAlign, WhiteSpace,
};''')

s = s.replace('''pub struct LayoutEngine<'a> {
    pub doc: &'a Document,
    pub styles: &'a [ComputedStyle],
    pub fonts: &'a mut FontStore,
    pub images: &'a HashMap<String, ImageData>,
    pub viewport: (f32, f32),
    pub base_url: String,
}''', '''#[derive(Clone, Copy, Debug)]
pub struct FloatInfo {
    pub x: f32,
    pub y: f32,
    pub w: f32,
    pub h: f32,
    pub side: Float,
}

pub struct LayoutEngine<'a> {
    pub doc: &'a Document,
    pub styles: &'a [ComputedStyle],
    pub fonts: &'a mut FontStore,
    pub images: &'a HashMap<String, ImageData>,
    pub viewport: (f32, f32),
    pub base_url: String,
    floats: Vec<FloatInfo>,
}''')
s = s.replace('''        LayoutEngine { doc, styles, fonts, images, viewport, base_url: base_url.to_string() }''',
'''        LayoutEngine { doc, styles, fonts, images, viewport, base_url: base_url.to_string(), floats: Vec::new() }''')

# float handling in layout_flow
s = s.replace('''            let inline = b.children[i].is_inline_level();
            if inline {''', '''            if b.children[i].style.float != Float::None {
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
                    // Stack below overlapping floats on the same side.
                    for f in &self.floats {
                        if f.side == side && fy < f.y + f.h && fy + h > f.y {
                            fy = fy.max(f.y + f.h);
                        }
                    }
                    let target_x = match side {
                        Float::Left => cx,
                        Float::Right => cx + inner_w - w,
                        Float::None => child.rect.x,
                    };
                    let dx = target_x - child.rect.x;
                    let dy = fy - child.rect.y;
                    child.shift(dx, dy);
                    self.floats.push(FloatInfo { x: child.rect.x, y: child.rect.y, w, h: child.rect.h, side });
                    state.cursor = state.cursor.max(child.rect.y + child.rect.h);
                    b.children[i] = child;
                }
                i += 1;
                continue;
            }
            let inline = b.children[i].is_inline_level();
            if inline {''')

# floats scope around flow: record count at start and trim at end; easiest: in layout_flow keep added floats until end
s = s.replace('''    fn layout_flow(&mut self, b: &mut LayoutBox, cx: f32, cy: f32, inner_w: f32) -> f32 {
        let mut state = FlowState''', '''    fn layout_flow(&mut self, b: &mut LayoutBox, cx: f32, cy: f32, inner_w: f32) -> f32 {
        let float_mark = self.floats.len();
        let mut state = FlowState''')
s = s.replace('''        (state.cursor - cy).max(0.0)
    }

    fn resolve_margin''', '''        // Floats extend the flow height and stop affecting siblings above.
        for f in &self.floats[float_mark..] {
            state.cursor = state.cursor.max(f.y + f.h);
        }
        self.floats.truncate(float_mark);
        (state.cursor - cy).max(0.0)
    }

    fn resolve_margin''')

# line area helper
s = s.replace('''    fn resolve_margin(&self, l: Length, basis: f32) -> f32 {''', '''    fn line_area(&self, y: f32, cx: f32, max_w: f32, indent: f32, line_h: f32) -> (f32, f32) {
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

    fn resolve_margin(&self, l: Length, basis: f32) -> f32 {''')
open(p, 'w', encoding='utf-8').write(s)
print('float plumbing patched')
