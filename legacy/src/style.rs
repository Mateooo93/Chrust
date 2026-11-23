#![allow(dead_code)]
//! CSS cascade, inheritance and computed styles.

use std::collections::HashMap;
use std::sync::OnceLock;

use crate::css::{
    media_matches, parse_color_value, parse_declarations, parse_font_size, parse_length,
    parse_stylesheet, Declaration, Length, MatchContext, Rule, Selector, Simple, StyleSheet,
};
use crate::dom::{Document, NodeId};
use crate::util::{parse_color, clamp, Color};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Display {
    None,
    Block,
    Inline,
    InlineBlock,
    ListItem,
    Table,
    TableRow,
    TableCell,
    TableRowGroup,
    TableCaption,
    Flex,
    InlineFlex,
    Grid,
    InlineGrid,
}

impl Display {
    pub fn is_block_level(self) -> bool {
        matches!(
            self,
            Display::Block
                | Display::ListItem
                | Display::Table
                | Display::TableRow
                | Display::TableCell
                | Display::TableRowGroup
                | Display::TableCaption
                | Display::Flex
                | Display::Grid
        )
    }
    pub fn is_inline_level(self) -> bool {
        matches!(self, Display::Inline | Display::InlineBlock | Display::InlineFlex | Display::InlineGrid)
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Position {
    Static,
    Relative,
    Absolute,
    Fixed,
    Sticky,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum BorderStyle {
    None,
    Solid,
    Dashed,
    Dotted,
    Double,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TextAlign {
    Left,
    Center,
    Right,
    Justify,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum WhiteSpace {
    Normal,
    Nowrap,
    Pre,
    PreWrap,
    PreLine,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TextTransform {
    None,
    Uppercase,
    Lowercase,
    Capitalize,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct TextDecoration {
    pub underline: bool,
    pub line_through: bool,
    pub overline: bool,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum FlexDirection {
    Row,
    RowReverse,
    Column,
    ColumnReverse,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum JustifyContent {
    Start,
    Center,
    End,
    SpaceBetween,
    SpaceAround,
    SpaceEvenly,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum AlignItems {
    Stretch,
    Start,
    Center,
    End,
    Baseline,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ListStyleType {
    Disc,
    Circle,
    Square,
    Decimal,
    DecimalLeadingZero,
    LowerAlpha,
    UpperAlpha,
    LowerRoman,
    UpperRoman,
    None,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Float {
    None,
    Left,
    Right,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub enum VerticalAlign {
    Baseline,
    Top,
    Middle,
    Bottom,
    Sub,
    Super,
    TextTop,
    TextBottom,
    Px(f32),
}

#[derive(Clone, Debug, PartialEq)]
pub struct Gradient {
    pub angle: f32,
    pub stops: Vec<(Color, f32)>,
}

/// Generated content from a `::before` / `::after` rule.
#[derive(Clone, Debug)]
pub struct PseudoContent {
    pub content: String,
    pub style: Box<ComputedStyle>,
}

#[derive(Clone, Debug)]
pub struct ComputedStyle {
    pub display: Display,
    pub position: Position,
    pub color: Color,
    pub background_color: Color,
    pub background_image: Option<String>,
    pub mask_image: Option<String>,
    pub bg_size: String,
    pub bg_size_px: Option<(f32, f32)>,
    pub bg_pos_px_x: Option<f32>,
    pub bg_pos_px_y: Option<f32>,
    pub bg_pos_x: f32,
    pub bg_pos_y: f32,
    pub bg_no_repeat: bool,
    pub background_gradient: Option<Gradient>,
    pub font_family: String,
    pub font_size: f32,
    pub font_weight: u16,
    pub font_italic: bool,
    pub line_height: f32,
    pub line_height_factor: f32,
    pub text_align: TextAlign,
    pub text_decoration: TextDecoration,
    pub white_space: WhiteSpace,
    pub text_transform: TextTransform,
    pub letter_spacing: f32,
    pub word_spacing: f32,
    pub text_indent: f32,
    pub margin: [Length; 4],
    pub padding: [Length; 4],
    pub border_width: [f32; 4],
    pub border_style: [BorderStyle; 4],
    pub border_color: [Color; 4],
    pub width: Length,
    pub height: Length,
    pub min_width: Length,
    pub max_width: Length,
    pub min_height: Length,
    pub max_height: Length,
    pub box_sizing_border: bool,
    pub border_radius: f32,
    pub flex_direction: FlexDirection,
    pub flex_wrap: bool,
    pub justify_content: JustifyContent,
    pub align_items: AlignItems,
    pub align_self: AlignItems,
    pub flex_grow: f32,
    pub flex_shrink: f32,
    pub flex_basis: Length,
    pub gap: f32,
    pub row_gap: f32,
    pub grid_template_columns: String,
    pub grid_template_rows: String,
    pub grid_auto_rows: String,
    pub grid_template_areas: Vec<Vec<String>>,
    pub grid_area: String,
    pub grid_column: String,
    pub grid_row: String,
    pub list_style_type: ListStyleType,
    pub opacity: f32,
    pub float: Float,
    pub overflow_hidden: bool,
    pub before: Option<Box<PseudoContent>>,
    pub after: Option<Box<PseudoContent>>,
    pub vertical_align: VerticalAlign,
    pub z_index: i32,
    pub visible: bool,
    pub cursor_pointer: bool,
    pub custom: HashMap<String, String>,
}

impl Default for ComputedStyle {
    fn default() -> Self {
        ComputedStyle {
            display: Display::Inline,
            position: Position::Static,
            color: Color::BLACK,
            background_color: Color::TRANSPARENT,
            background_image: None,
            mask_image: None,
            bg_size: "auto".to_string(),
            bg_size_px: None,
            bg_pos_px_x: None,
            bg_pos_px_y: None,
            bg_pos_x: 0.0,
            bg_pos_y: 0.0,
            bg_no_repeat: false,
            background_gradient: None,
            font_family: "Segoe UI".to_string(),
            font_size: 16.0,
            font_weight: 400,
            font_italic: false,
            line_height: 19.2,
            line_height_factor: 1.2,
            text_align: TextAlign::Left,
            text_decoration: TextDecoration::default(),
            white_space: WhiteSpace::Normal,
            text_transform: TextTransform::None,
            letter_spacing: 0.0,
            word_spacing: 0.0,
            text_indent: 0.0,
            margin: [Length::Px(0.0); 4],
            padding: [Length::Px(0.0); 4],
            border_width: [0.0; 4],
            border_style: [BorderStyle::None; 4],
            border_color: [Color::BLACK; 4],
            width: Length::Auto,
            height: Length::Auto,
            min_width: Length::Auto,
            max_width: Length::Auto,
            min_height: Length::Auto,
            max_height: Length::Auto,
            box_sizing_border: false,
            border_radius: 0.0,
            flex_direction: FlexDirection::Row,
            flex_wrap: false,
            justify_content: JustifyContent::Start,
            align_items: AlignItems::Stretch,
            align_self: AlignItems::Stretch,
            flex_grow: 0.0,
            flex_shrink: 1.0,
            flex_basis: Length::Auto,
            gap: 0.0,
            row_gap: 0.0,
            grid_template_columns: String::new(),
            grid_template_rows: String::new(),
            grid_auto_rows: String::new(),
            grid_template_areas: Vec::new(),
            grid_area: String::new(),
            grid_column: String::new(),
            grid_row: String::new(),
            list_style_type: ListStyleType::Disc,
            opacity: 1.0,
            float: Float::None,
            overflow_hidden: false,
            before: None,
            after: None,
            vertical_align: VerticalAlign::Baseline,
            z_index: 0,
            visible: true,
            cursor_pointer: false,
            custom: HashMap::new(),
        }
    }
}

impl ComputedStyle {
    pub fn inherited_from(parent: &ComputedStyle) -> ComputedStyle {
        let mut s = ComputedStyle::default();
        s.color = parent.color;
        s.font_family = parent.font_family.clone();
        s.font_size = parent.font_size;
        s.font_weight = parent.font_weight;
        s.font_italic = parent.font_italic;
        s.line_height = parent.line_height;
        s.line_height_factor = parent.line_height_factor;
        s.text_align = parent.text_align;
        s.white_space = parent.white_space;
        s.text_transform = parent.text_transform;
        s.letter_spacing = parent.letter_spacing;
        s.word_spacing = parent.word_spacing;
        s.text_indent = parent.text_indent;
        s.list_style_type = parent.list_style_type;
        s.visible = parent.visible;
        s.cursor_pointer = parent.cursor_pointer;
        s.custom = parent.custom.clone();
        s.vertical_align = parent.vertical_align;
        s
    }

    pub fn border_box_extra(&self, basis: f32) -> (f32, f32) {
        let mut h = 0.0;
        let mut v = 0.0;
        for i in 0..4 {
            let w = self.margin[i];
            let p = self.padding[i];
            let b = self.border_width[i];
            let val = crate::css::resolve_length(p, basis).unwrap_or(0.0) + b;
            if i % 2 == 0 {
                v += val;
            } else {
                h += val;
            }
            let _ = w;
        }
        (h, v)
    }
}

const UA_CSS: &str = r#"
html, body, div, p, h1, h2, h3, h4, h5, h6, ul, ol, li, dl, dt, dd,
header, footer, section, article, aside, nav, main, figure, figcaption,
form, fieldset, blockquote, pre, hr, address, details, summary, center,
table, caption, thead, tbody, tfoot, tr, td, th, noscript, template, iframe { display: block; }
head, title, meta, link, style, script, base, basefont, param, source, track, svg, object, embed, canvas { display: none; }
body { margin: 8px; }
p { margin: 1em 0; }
h1 { font-size: 2em; margin: 0.67em 0; font-weight: bold; }
h2 { font-size: 1.5em; margin: 0.83em 0; font-weight: bold; }
h3 { font-size: 1.17em; margin: 1em 0; font-weight: bold; }
h4 { font-size: 1em; margin: 1.33em 0; font-weight: bold; }
h5 { font-size: 0.83em; margin: 1.67em 0; font-weight: bold; }
h6 { font-size: 0.67em; margin: 2.33em 0; font-weight: bold; }
ul, ol { margin: 1em 0; padding-left: 40px; }
ul ul, ol ol, ul ol, ol ul { margin: 0; }
li { display: list-item; }
dl { margin: 1em 0; }
dd { margin-left: 40px; }
blockquote { margin: 1em 40px; }
figure { margin: 1em 40px; }
pre, code, kbd, samp, tt { font-family: Consolas, monospace; }
pre { white-space: pre; margin: 1em 0; }
code { font-size: 0.95em; }
hr { border: 0; border-top: 1px solid #999999; height: 0; margin: 0.5em auto; }
a { color: #0000ee; text-decoration: underline; cursor: pointer; }
a:visited { color: #551a8b; }
b, strong { font-weight: bold; }
i, em, cite, var, address { font-style: italic; }
u, ins { text-decoration: underline; }
s, strike, del { text-decoration: line-through; }
small { font-size: 0.83em; }
big { font-size: 1.17em; }
sup { vertical-align: super; font-size: 0.83em; }
sub { vertical-align: sub; font-size: 0.83em; }
mark { background-color: yellow; color: black; }
center { text-align: center; }
table { display: table; border-collapse: separate; border-spacing: 2px; }
thead { display: table-row-group; }
tbody { display: table-row-group; }
tfoot { display: table-row-group; }
tr { display: table-row; }
td, th { display: table-cell; padding: 1px; vertical-align: middle; }
th { font-weight: bold; text-align: center; }
caption { display: table-caption; text-align: center; }
img { display: inline-block; }
input { display: inline-block; width: 170px; height: 21px; border: 1px solid #767676; padding: 1px 3px; background-color: white; font-size: 13.333333px; vertical-align: middle; }
input[type=checkbox], input[type=radio] { width: 13px; height: 13px; padding: 0; border: 1px solid #767676; }
input[type=submit], input[type=button], input[type=reset], button { display: inline-block; width: auto; height: 24px; padding: 1px 8px; border: 1px solid #767676; background-color: #efefef; font-size: 13.333333px; text-align: center; }
button { vertical-align: middle; }
textarea { display: inline-block; width: 170px; height: 60px; border: 1px solid #767676; padding: 2px; background-color: white; font-family: Consolas, monospace; font-size: 13.333333px; white-space: pre-wrap; }
select { display: inline-block; width: auto; height: 21px; border: 1px solid #767676; background-color: white; font-size: 13.333333px; padding: 0 2px; }
label, span, small, strong, b, i, em, a, code, kbd, samp, sub, sup, u, s, strike, del, ins, mark, abbr, acronym, q, cite, dfn, time, bdi, bdo, ruby, rt, rp, wbr { display: inline; }
fieldset { border: 1px solid #c0c0c0; margin: 0 2px; padding: 0.35em 0.75em 0.625em; }
legend { padding: 0 2px; font-weight: bold; }
details > summary { display: block; }
"#;

fn ua_sheet() -> &'static StyleSheet {
    static UA: OnceLock<StyleSheet> = OnceLock::new();
    UA.get_or_init(|| parse_stylesheet(UA_CSS))
}

pub fn default_display_for_tag(tag: &str) -> Display {
    match tag {
        "head" | "title" | "meta" | "link" | "style" | "script" | "base" | "noscript"
        | "template" | "canvas" | "object" | "embed" | "param" | "source" | "track" => {
            Display::None
        }
        "svg" => Display::InlineBlock,
        "table" => Display::Table,
        "thead" | "tbody" | "tfoot" => Display::TableRowGroup,
        "tr" => Display::TableRow,
        "td" | "th" => Display::TableCell,
        "caption" => Display::TableCaption,
        "li" => Display::ListItem,
        "img" | "input" | "textarea" | "select" | "button" => Display::InlineBlock,
        t if crate::dom::is_block_tag(t) => Display::Block,
        _ => Display::Inline,
    }
}

#[derive(Clone)]
struct Priority {
    important: bool,
    origin: u8,
    spec: (u32, u32, u32),
    order: usize,
}

impl Priority {
    fn key(&self) -> (bool, u8, (u32, u32, u32), usize) {
        (self.important, self.origin, self.spec, self.order)
    }
}

struct Winners {
    map: HashMap<String, (String, Priority)>,
}

impl Winners {
    fn new() -> Winners {
        Winners { map: HashMap::new() }
    }
    fn insert(&mut self, name: &str, value: &str, prio: &Priority) {
        if let Some((_, existing)) = self.map.get(name) {
            if existing.key() > prio.key() {
                return;
            }
        }
        self.map.insert(name.to_string(), (value.to_string(), prio.clone()));
    }
    fn get(&self, name: &str) -> Option<&str> {
        self.map.get(name).map(|(v, _)| v.as_str())
    }
    fn apply_shorthand(&mut self, decl: &Declaration, prio: &Priority) {
        let name = decl.name.as_str();
        let value = decl.value.trim();
        match name {
            "margin" | "padding" | "border-width" | "border-style" | "border-color" => {
                let parts = split_space_values(value);
                if parts.is_empty() || parts.len() > 4 {
                    return;
                }
                let (t, r, b, l) = match parts.len() {
                    1 => (0, 0, 0, 0),
                    2 => (0, 1, 0, 1),
                    3 => (0, 1, 2, 1),
                    _ => (0, 1, 2, 3),
                };
                let names = if name == "margin" {
                    ["margin-top", "margin-right", "margin-bottom", "margin-left"]
                } else if name == "padding" {
                    ["padding-top", "padding-right", "padding-bottom", "padding-left"]
                } else if name == "border-width" {
                    ["border-top-width", "border-right-width", "border-bottom-width", "border-left-width"]
                } else if name == "border-style" {
                    ["border-top-style", "border-right-style", "border-bottom-style", "border-left-style"]
                } else {
                    ["border-top-color", "border-right-color", "border-bottom-color", "border-left-color"]
                };
                for (prop_idx, part_idx) in [(0usize, t), (1, r), (2, b), (3, l)] {
                    self.insert(names[prop_idx], &parts[part_idx], prio);
                }
            }
            "border" | "border-top" | "border-right" | "border-bottom" | "border-left" => {
                let (width, style, color) = parse_border_shorthand(value);
                let sides: Vec<usize> = match name {
                    "border" => vec![0, 1, 2, 3],
                    "border-top" => vec![0],
                    "border-right" => vec![1],
                    "border-bottom" => vec![2],
                    "border-left" => vec![3],
                    _ => vec![],
                };
                let side_names = ["top", "right", "bottom", "left"];
                for side in sides {
                    if let Some(w) = &width {
                        self.insert(&format!("border-{}-width", side_names[side]), w, prio);
                    }
                    if let Some(s) = &style {
                        self.insert(&format!("border-{}-style", side_names[side]), s, prio);
                    }
                    if let Some(c) = &color {
                        self.insert(&format!("border-{}-color", side_names[side]), c, prio);
                    }
                }
                if name == "border" {
                    if let Some(s) = &style {
                        self.insert("border-image", "none", prio);
                        let _ = s;
                    }
                }
            }
            "background" => {
                let mut color = None;
                let mut image = None;
                let mut size = None;
                let mut repeat = None;
                let mut pos = Vec::new();
                for token in split_space_values_keep_functions(value) {
                    let low = token.to_ascii_lowercase();
                    if low.starts_with("url(") || low.contains("gradient(") {
                        image = Some(token);
                    } else if parse_color(&token).is_some()
                        || low == "transparent"
                    {
                        color = Some(token);
                    } else if low == "no-repeat" || low == "repeat-x" || low == "repeat-y" {
                        repeat = Some(low);
                    } else if low == "cover" || low == "contain" {
                        size = Some(low);
                    } else if matches!(low.as_str(), "left" | "right" | "top" | "bottom" | "center")
                        || low.ends_with('%')
                        || low == "/"
                    {
                        pos.push(token);
                    }
                }
                if let Some(rep) = repeat {
                    self.insert("background-repeat", &rep, prio);
                }
                if let Some(sz) = size {
                    self.insert("background-size", &sz, prio);
                }
                if !pos.is_empty() {
                    self.insert("background-position", &pos.join(" "), prio);
                }
                if let Some(c) = color {
                    self.insert("background-color", &c, prio);
                } else {
                    self.insert("background-color", "transparent", prio);
                }
                if let Some(i) = image {
                    self.insert("background-image", &i, prio);
                }
            }
            "background-color" => self.insert("background-color", value, prio),
            "font" => {
                if let Some((style, weight, size, line_height, family)) = parse_font_shorthand(value) {
                    if let Some(st) = style {
                        self.insert("font-style", &st, prio);
                    }
                    if let Some(w) = weight {
                        self.insert("font-weight", &w, prio);
                    }
                    if let Some(sz) = size {
                        self.insert("font-size", &sz, prio);
                    }
                    if let Some(lh) = line_height {
                        self.insert("line-height", &lh, prio);
                    }
                    if let Some(f) = family {
                        self.insert("font-family", &f, prio);
                    }
                }
            }
            "text-decoration" => {
                let mut lines: Vec<&str> = Vec::new();
                if value.contains("underline") {
                    lines.push("underline");
                }
                if value.contains("line-through") {
                    lines.push("line-through");
                }
                if value.contains("overline") {
                    lines.push("overline");
                }
                if lines.is_empty() {
                    lines.push("none");
                }
                self.insert("text-decoration-line", &lines.join(" "), prio);
            }
            "list-style" => {
                for token in split_space_values(value) {
                    if token == "inside" || token == "outside" {
                        self.insert("list-style-position", &token, prio);
                    } else if token.starts_with("url(") {
                        self.insert("list-style-image", &token, prio);
                    } else {
                        self.insert("list-style-type", &token, prio);
                    }
                }
            }
            "flex" => {
                let parts = split_space_values(value);
                if parts.len() == 1 {
                    match parts[0].as_str() {
                        "none" => {
                            self.insert("flex-grow", "0", prio);
                            self.insert("flex-shrink", "0", prio);
                            self.insert("flex-basis", "auto", prio);
                        }
                        "auto" => {
                            self.insert("flex-grow", "1", prio);
                            self.insert("flex-shrink", "1", prio);
                            self.insert("flex-basis", "auto", prio);
                        }
                        "initial" => {
                            self.insert("flex-grow", "0", prio);
                            self.insert("flex-shrink", "1", prio);
                            self.insert("flex-basis", "auto", prio);
                        }
                        other => {
                            if let Ok(n) = other.parse::<f32>() {
                                self.insert("flex-grow", &format!("{}", n), prio);
                                self.insert("flex-shrink", "1", prio);
                                self.insert("flex-basis", "0%", prio);
                            } else {
                                self.insert("flex-basis", other, prio);
                            }
                        }
                    }
                } else {
                    for (i, part) in parts.iter().enumerate() {
                        match i {
                            0 => self.insert("flex-grow", part, prio),
                            1 => self.insert("flex-shrink", part, prio),
                            _ => self.insert("flex-basis", part, prio),
                        }
                    }
                }
            }
            "flex-flow" => {
                for token in split_space_values(value) {
                    if token.contains("wrap") {
                        self.insert("flex-wrap", &token, prio);
                    } else {
                        self.insert("flex-direction", &token, prio);
                    }
                }
            }
            "gap" => {
                let parts = split_space_values(value);
                if parts.len() == 1 {
                    self.insert("row-gap", &parts[0], prio);
                    self.insert("column-gap", &parts[0], prio);
                } else if parts.len() >= 2 {
                    self.insert("row-gap", &parts[0], prio);
                    self.insert("column-gap", &parts[1], prio);
                }
            }
            "overflow" => {
                let first = split_space_values(value).into_iter().next().unwrap_or_default();
                self.insert("overflow", &first, prio);
            }
            "place-items" | "place-content" => {
                let parts = split_space_values(value);
                if let Some(p) = parts.first() {
                    self.insert("align-items", p, prio);
                }
                if let Some(p) = parts.get(1) {
                    self.insert("justify-content", p, prio);
                }
            }
            "inset" => {
                let parts = split_space_values(value);
                if parts.is_empty() || parts.len() > 4 {
                    return;
                }
                let (t, r, b, l) = match parts.len() {
                    1 => (0, 0, 0, 0),
                    2 => (0, 1, 0, 1),
                    3 => (0, 1, 2, 1),
                    _ => (0, 1, 2, 3),
                };
                let names = ["top", "right", "bottom", "left"];
                for (prop_idx, part_idx) in [(0usize, t), (1, r), (2, b), (3, l)] {
                    self.insert(names[prop_idx], &parts[part_idx], prio);
                }
            }
            "border-radius" => {
                let first = split_space_values(value).into_iter().next().unwrap_or_default();
                self.insert("border-radius", &first, prio);
            }
            _ => {
                let normalized = normalize_logical_property(name);
                self.insert(&normalized, value, prio);
            }
        }
        // Also store custom properties / normal properties not handled above.
        if !matches!(
            name,
            "margin" | "padding" | "border" | "border-width" | "border-style" | "border-color"
                | "border-top" | "border-right" | "border-bottom" | "border-left" | "background"
                | "font" | "text-decoration" | "list-style" | "flex" | "flex-flow" | "gap"
                | "overflow" | "place-items" | "place-content" | "inset" | "border-radius"
        ) {
            let normalized = normalize_logical_property(name);
            self.insert(&normalized, value, prio);
        }
    }
}

fn normalize_logical_property(name: &str) -> String {
    match name {
        "margin-inline-start" => "margin-left".into(),
        "margin-inline-end" => "margin-right".into(),
        "margin-block-start" => "margin-top".into(),
        "margin-block-end" => "margin-bottom".into(),
        "padding-inline-start" => "padding-left".into(),
        "padding-inline-end" => "padding-right".into(),
        "padding-block-start" => "padding-top".into(),
        "padding-block-end" => "padding-bottom".into(),
        "inset-inline-start" => "left".into(),
        "inset-inline-end" => "right".into(),
        "inset-block-start" => "top".into(),
        "inset-block-end" => "bottom".into(),
        "border-inline-start" => "border-left".into(),
        "border-inline-end" => "border-right".into(),
        "min-inline-size" => "min-width".into(),
        "max-inline-size" => "max-width".into(),
        "inline-size" => "width".into(),
        "block-size" => "height".into(),
        "background-color" => "background-color".into(),
        other => other.to_string(),
    }
}

fn split_space_values(s: &str) -> Vec<String> {
    // Split on whitespace but keep function arguments together.
    split_space_values_keep_functions(s)
}

fn split_space_values_keep_functions(s: &str) -> Vec<String> {
    let mut out = Vec::new();
    let mut cur = String::new();
    let mut depth = 0;
    let mut quote: Option<char> = None;
    for c in s.chars() {
        if let Some(q) = quote {
            cur.push(c);
            if c == q {
                quote = None;
            }
            continue;
        }
        match c {
            '"' | '\'' => {
                quote = Some(c);
                cur.push(c);
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
    }
    if !cur.is_empty() {
        out.push(cur);
    }
    out
}

fn parse_border_shorthand(value: &str) -> (Option<String>, Option<String>, Option<String>) {
    let mut width = None;
    let mut style = None;
    let mut color = None;
    for token in split_space_values(value) {
        let low = token.to_ascii_lowercase();
        if matches!(low.as_str(), "none" | "hidden" | "solid" | "dashed" | "dotted" | "double" | "groove" | "ridge" | "inset" | "outset") {
            style = Some(low);
        } else if matches!(low.as_str(), "thin" | "medium" | "thick")
            || parse_length(&low, 16.0, 16.0, (1000.0, 800.0)).is_some()
        {
            width = Some(token);
        } else if parse_color(&token).is_some() || low == "currentcolor" || low == "transparent" {
            color = Some(token);
        }
    }
    (width, style, color)
}

fn parse_font_shorthand(value: &str) -> Option<(Option<String>, Option<String>, Option<String>, Option<String>, Option<String>)> {
    let v = value.trim();
    if matches!(
        v.to_ascii_lowercase().as_str(),
        "caption" | "icon" | "menu" | "message-box" | "small-caption" | "status-bar" | "inherit" | "initial"
    ) {
        return None;
    }
    let parts = split_space_values(v);
    let mut style = None;
    let mut weight = None;
    let mut size = None;
    let mut line_height = None;
    let mut family = None;
    let mut i = 0;
    while i < parts.len() {
        let p = &parts[i];
        let low = p.to_ascii_lowercase();
        if (low.contains("px") || low.contains("pt") || low.contains("em") || low.contains("rem") || low.contains('%') || low.ends_with("vw") || low.ends_with("vh")) && size.is_none() {
            if let Some((sz, lh)) = p.split_once('/') {
                size = Some(sz.to_string());
                line_height = Some(lh.to_string());
                i += 1;
                if i < parts.len() {
                    family = Some(parts[i..].join(" "));
                }
            } else {
                size = Some(p.clone());
                i += 1;
                if i < parts.len() && parts[i].starts_with('/') {
                    line_height = Some(parts[i].trim_start_matches('/').to_string());
                    i += 1;
                }
                if i < parts.len() {
                    family = Some(parts[i..].join(" "));
                }
            }
            break;
        }
        if matches!(low.as_str(), "italic" | "oblique" | "normal") {
            if style.is_none() {
                style = Some(low);
            }
        } else if matches!(low.as_str(), "bold" | "bolder" | "lighter") || low.parse::<u16>().is_ok() {
            weight = Some(low);
        }
        i += 1;
    }
    Some((style, weight, size, line_height, family))
}

fn default_list_style(tag: &str) -> ListStyleType {
    match tag {
        "ol" => ListStyleType::Decimal,
        _ => ListStyleType::Disc,
    }
}

pub struct StyleResult {
    pub styles: Vec<ComputedStyle>,
}

fn property_basis(viewport: (f32, f32)) -> f32 {
    viewport.0.max(1.0)
}

#[derive(Default)]
struct RuleBuckets {
    id: HashMap<String, Vec<(usize, usize)>>,
    class: HashMap<String, Vec<(usize, usize)>>,
    tag: HashMap<String, Vec<(usize, usize)>>,
    universal: Vec<(usize, usize)>,
}

impl RuleBuckets {
    fn add(&mut self, key: BucketKey, rule: usize, sel: usize) {
        match key {
            BucketKey::Id(v) => self.id.entry(v).or_default().push((rule, sel)),
            BucketKey::Class(v) => self.class.entry(v).or_default().push((rule, sel)),
            BucketKey::Tag(v) => self.tag.entry(v).or_default().push((rule, sel)),
            BucketKey::Universal => self.universal.push((rule, sel)),
        }
    }

    fn candidates(&self, doc: &Document, node: NodeId) -> Vec<(usize, usize)> {
        let mut out = self.universal.clone();
        if let Some(id) = doc.attr(node, "id") {
            if let Some(v) = self.id.get(id) {
                out.extend_from_slice(v);
            }
        }
        if let Some(class) = doc.attr(node, "class") {
            for c in class.split_whitespace() {
                if let Some(v) = self.class.get(c) {
                    out.extend_from_slice(v);
                }
            }
        }
        if let Some(tag) = doc.tag(node) {
            if let Some(v) = self.tag.get(tag) {
                out.extend_from_slice(v);
            }
        }
        out
    }
}

enum BucketKey {
    Id(String),
    Class(String),
    Tag(String),
    Universal,
}

fn bucket_key(sel: &Selector) -> BucketKey {
    let comp = match sel.compounds.last() {
        Some(c) => c,
        None => return BucketKey::Universal,
    };
    let mut id = None;
    let mut class = None;
    let mut tag = None;
    for simple in &comp.simples {
        match simple {
            Simple::Id(v) => id = Some(v.clone()),
            Simple::Class(v) => class = Some(v.clone()),
            Simple::Type(v) => tag = Some(v.clone()),
            _ => {}
        }
    }
    if let Some(v) = id {
        BucketKey::Id(v)
    } else if let Some(v) = class {
        BucketKey::Class(v)
    } else if let Some(v) = tag {
        BucketKey::Tag(v)
    } else {
        BucketKey::Universal
    }
}

fn build_buckets(sheet: &StyleSheet, pseudo: Option<&str>) -> RuleBuckets {
    let mut buckets = RuleBuckets::default();
    for (ri, rule) in sheet.rules.iter().enumerate() {
        for (si, sel) in rule.selectors.iter().enumerate() {
            let p = sel
                .compounds
                .last()
                .and_then(|c| c.pseudo_element.as_deref());
            let keep = match pseudo {
                Some(which) => p == Some(which),
                None => p.is_none(),
            };
            if keep {
                buckets.add(bucket_key(sel), ri, si);
            }
        }
    }
    buckets
}

pub fn compute_styles(
    doc: &Document,
    author: &StyleSheet,
    viewport: (f32, f32),
    hovered: Option<NodeId>,
    active: Option<NodeId>,
    focused_input: Option<NodeId>,
) -> Vec<ComputedStyle> {
    let ua = ua_sheet();
    let ctx = MatchContext { doc, hovered, active, focused: focused_input };
    let author_index = build_buckets(author, None);
    let mut styles: Vec<ComputedStyle> = vec![ComputedStyle::default(); doc.nodes.len()];
    let root_fs = 16.0f32;
    let mut order = 0usize;
    let walk = doc.walk(doc.root);
    for node in walk {
        let parent_style = doc.nodes[node]
            .parent
            .map(|p| styles[p].clone())
            .unwrap_or_default();
        let el = match doc.element(node) {
            Some(e) => e,
            None => {
                // Text nodes take their parent's style.
                styles[node] = parent_style;
                continue;
            }
        };
        let tag = el.tag.clone();
        let mut style = if doc.nodes[node].parent.map(|p| p == doc.root).unwrap_or(false) {
            let mut s = ComputedStyle::inherited_from(&ComputedStyle::default());
            s.font_size = 16.0;
            s.line_height = 19.2;
            s
        } else {
            ComputedStyle::inherited_from(&parent_style)
        };
        style.display = default_display_for_tag(&tag);
        style.line_height_factor = parent_style.line_height_factor;
        if tag == "ol" || tag == "ul" {
            style.list_style_type = default_list_style(&tag);
        }

        let mut winners = Winners::new();
        // UA rules.
        for rule in &ua.rules {
            if let Some(media) = &rule.media {
                if !media_matches(media, viewport) {
                    continue;
                }
            }
            for sel in &rule.selectors {
                if sel.matches(node, &ctx) {
                    for d in &rule.decls {
                        let prio = Priority {
                            important: d.important,
                            origin: 0,
                            spec: sel.specificity(),
                            order: rule.order,
                        };
                        winners.apply_shorthand(d, &prio);
                    }
                }
            }
        }
        // Author rules (indexed by the rightmost compound for speed).
        for (ri, si) in author_index.candidates(doc, node) {
            let rule = &author.rules[ri];
            if let Some(media) = &rule.media {
                if !media_matches(media, viewport) {
                    continue;
                }
            }
            let sel = &rule.selectors[si];
            if sel.matches(node, &ctx) {
                for d in &rule.decls {
                    let prio = Priority {
                        important: d.important,
                        origin: 1,
                        spec: sel.specificity(),
                        order: order + rule.order,
                    };
                    winners.apply_shorthand(d, &prio);
                }
            }
        }
        order += author.rules.len() + 1;
        // Inline style.
        if let Some(attr) = el.attr("style") {
            let decls = parse_declarations(attr);
            for (i, d) in decls.iter().enumerate() {
                let prio = Priority {
                    important: d.important,
                    origin: 2,
                    spec: (u32::MAX, 0, 0),
                    order: i,
                };
                winners.apply_shorthand(d, &prio);
            }
        }

        // Custom properties (inherit from parent, then apply winners).
        for (name, (value, _)) in &winners.map {
            if name.starts_with("--") {
                style.custom.insert(name.clone(), value.clone());
            }
        }

        apply_winners(&mut style, &winners, &tag, viewport, parent_style.font_size, root_fs);
        if (tag == "td" || tag == "th") && winners.get("text-align").is_none() {
            style.text_align = TextAlign::Left;
        }
        apply_html_attributes(doc, node, &mut style, viewport);
        if style.line_height_factor <= 0.0 {
            style.line_height_factor = 1.2;
        }
        style.line_height = (style.line_height_factor * style.font_size).max(style.font_size * 1.05);
        if style.font_size <= 0.0 {
            style.font_size = 16.0;
            style.line_height = 19.2;
        }
        styles[node] = style;
    }
    // Generated ::before / ::after content.
    let walk = doc.walk(doc.root);
    for node in walk {
        if !doc.is_element(node) {
            continue;
        }
        let base = styles[node].clone();
        for which in ["before", "after"] {
            let mut winners = Winners::new();
            let buckets = build_buckets(author, Some(which));
            for (ri, si) in buckets.candidates(doc, node) {
                let rule = &author.rules[ri];
                if let Some(media) = &rule.media {
                    if !media_matches(media, viewport) {
                        continue;
                    }
                }
                let sel = &rule.selectors[si];
                if sel.matches_pseudo(node, &ctx, which) {
                    for d in &rule.decls {
                        let prio = Priority {
                            important: d.important,
                            origin: 1,
                            spec: sel.specificity(),
                            order: rule.order,
                        };
                        winners.apply_shorthand(d, &prio);
                    }
                }
            }
            let raw = match winners.get("content") {
                Some(c) => c,
                None => continue,
            };
            let text = parse_pseudo_content(&raw, doc, node);
            if text.is_empty() {
                continue;
            }
            let mut pstyle = base.clone();
            pstyle.before = None;
            pstyle.after = None;
            for (name, (value, _)) in &winners.map {
                if name.starts_with("--") {
                    pstyle.custom.insert(name.clone(), value.clone());
                }
            }
            let tag = doc.tag(node).unwrap_or("");
            apply_winners(&mut pstyle, &winners, tag, viewport, base.font_size, root_fs);
            if winners.get("display").is_none() {
                pstyle.display = Display::Inline;
            }
            let pc = PseudoContent { content: text, style: Box::new(pstyle) };
            if which == "before" {
                styles[node].before = Some(Box::new(pc));
            } else {
                styles[node].after = Some(Box::new(pc));
            }
        }
    }
    styles
}

/// Interpret a CSS `content` value (strings, attr(), quotes).
fn parse_pseudo_content(value: &str, doc: &Document, node: NodeId) -> String {
    let v = value.trim();
    if v.eq_ignore_ascii_case("none") || v.eq_ignore_ascii_case("normal") {
        return String::new();
    }
    let mut out = String::new();
    let chars: Vec<char> = v.chars().collect();
    let mut i = 0;
    while i < chars.len() {
        let c = chars[i];
        if c == '"' || c == '\'' {
            let quote = c;
            i += 1;
            while i < chars.len() && chars[i] != quote {
                if chars[i] == '\\' && i + 1 < chars.len() {
                    i += 1;
                    let e = chars[i];
                    out.push(match e {
                        'n' => '\n',
                        't' => '\t',
                        other => other,
                    });
                } else {
                    out.push(chars[i]);
                }
                i += 1;
            }
            i += 1;
        } else if i + 5 <= chars.len() && chars[i..i + 5].iter().copied().eq("attr(".chars()) {
            let mut j = i + 5;
            while j < chars.len() && chars[j] != ')' {
                j += 1;
            }
            let name: String = chars[i + 5..j].iter().collect();
            if let Some(val) = doc.attr(node, name.trim()) {
                out.push_str(val);
            }
            i = j + 1;
        } else {
            i += 1;
        }
    }
    out
}

fn resolve_var(value: &str, custom: &HashMap<String, String>, depth: u32) -> String {
    if depth > 8 || !value.contains("var(") {
        return value.to_string();
    }
    let mut out = String::new();
    let chars: Vec<char> = value.chars().collect();
    let mut i = 0;
    while i < chars.len() {
        if i + 4 <= chars.len() && chars[i..i + 4].iter().copied().eq("var(".chars()) {
            let mut j = i + 4;
            let mut depth_p = 1;
            while j < chars.len() && depth_p > 0 {
                if chars[j] == '(' {
                    depth_p += 1;
                } else if chars[j] == ')' {
                    depth_p -= 1;
                }
                j += 1;
            }
            let inner: String = chars[i + 4..j.saturating_sub(1)].iter().collect();
            let (name, fallback) = match inner.split_once(',') {
                Some((n, f)) => (n.trim().to_string(), Some(f.trim().to_string())),
                None => (inner.trim().to_string(), None),
            };
            let resolved = custom
                .get(&name)
                .cloned()
                .or(fallback)
                .unwrap_or_default();
            out.push_str(&resolve_var(&resolved, custom, depth + 1));
            i = j;
        } else {
            out.push(chars[i]);
            i += 1;
        }
    }
    out
}

fn apply_winners(
    style: &mut ComputedStyle,
    winners: &Winners,
    tag: &str,
    viewport: (f32, f32),
    parent_font_size: f32,
    root_fs: f32,
) {
    let custom_props = style.custom.clone();
    let get = |name: &str| -> Option<String> {
        winners.get(name).map(|v| resolve_var(v, &custom_props, 0))
    };
    // Display.
    if let Some(v) = get("display") {
        style.display = match v.trim().to_ascii_lowercase().as_str() {
            "none" => Display::None,
            "block" | "flow-root" => Display::Block,
            "inline" => Display::Inline,
            "inline-block" => Display::InlineBlock,
            "list-item" => Display::ListItem,
            "table" => Display::Table,
            "inline-table" => Display::InlineBlock,
            "table-row" => Display::TableRow,
            "table-cell" => Display::TableCell,
            "table-row-group" | "table-header-group" | "table-footer-group" => Display::TableRowGroup,
            "table-caption" => Display::TableCaption,
            "flex" => Display::Flex,
            "inline-flex" => Display::InlineFlex,
            "grid" => Display::Grid,
            "inline-grid" => Display::InlineGrid,
            "contents" => style.display,
            _ => style.display,
        };
    }
    // Position.
    if let Some(v) = get("position") {
        style.position = match v.trim().to_ascii_lowercase().as_str() {
            "relative" => Position::Relative,
            "absolute" => Position::Absolute,
            "fixed" => Position::Fixed,
            "sticky" => Position::Sticky,
            _ => Position::Static,
        };
    }
    // Font size first (em units depend on it).
    if let Some(v) = get("font-size") {
        if let Some(fs) = parse_font_size(&v, parent_font_size, root_fs, viewport) {
            style.font_size = fs.max(1.0);
        }
    }
    if let Some(v) = get("font-weight") {
        let low = v.trim().to_ascii_lowercase();
        style.font_weight = match low.as_str() {
            "normal" => 400,
            "bold" => 700,
            "bolder" => (style.font_weight + 300).min(900),
            "lighter" => style.font_weight.saturating_sub(300).max(100),
            _ => low.parse::<u16>().unwrap_or(style.font_weight),
        };
    }
    if let Some(v) = get("font-style") {
        style.font_italic = matches!(v.trim().to_ascii_lowercase().as_str(), "italic" | "oblique");
    }
    if let Some(v) = get("font-family") {
        style.font_family = v.trim().to_string();
    } else if tag == "input" || tag == "button" || tag == "select" || tag == "textarea" {
        // Form controls use the system UI font by default.
        style.font_family = "Segoe UI".to_string();
    }
    if let Some(v) = get("line-height") {
        let low = v.trim().to_ascii_lowercase();
        if low == "normal" {
            style.line_height_factor = 1.2;
        } else if let Ok(n) = low.parse::<f32>() {
            style.line_height_factor = n;
        } else if let Some(l) = parse_length(&low, style.font_size, root_fs, viewport) {
            match l {
                Length::Px(px) => style.line_height_factor = px / style.font_size.max(1.0),
                Length::Percent(p) => style.line_height_factor = p,
                Length::Auto => {}
            }
        }
    }
    if let Some(v) = get("color") {
        if let Some(c) = parse_color_value(&v, style.color) {
            style.color = c;
        }
    }
    if let Some(v) = get("background-color") {
        if let Some(c) = parse_color_value(&v, style.background_color) {
            style.background_color = c;
        }
    }
    if let Some(v) = get("background-size") {
        let low = v.trim().to_ascii_lowercase();
        if low.contains("cover") {
            style.bg_size = "cover".into();
            style.bg_size_px = None;
        } else if low.contains("contain") {
            style.bg_size = "contain".into();
            style.bg_size_px = None;
        } else {
            style.bg_size = "auto".into();
            let mut sizes: Vec<Option<f32>> = Vec::new();
            for tok in low.split_whitespace() {
                if let Some(p) = tok.strip_suffix("px") {
                    sizes.push(p.trim().parse::<f32>().ok());
                } else if tok == "auto" {
                    sizes.push(None);
                }
            }
            if sizes.len() >= 1 && sizes.iter().any(|s| s.is_some()) {
                let w = sizes.first().copied().flatten();
                let h = sizes.get(1).copied().flatten();
                if let Some(w) = w {
                    style.bg_size_px = Some((w, h.unwrap_or(-1.0)));
                } else if let Some(h) = h {
                    style.bg_size_px = Some((-1.0, h));
                }
            }
        }
    }
    if let Some(v) = get("background-position") {
        let low = v.trim().to_ascii_lowercase();
        let parts: Vec<&str> = low.split_whitespace().collect();
        let frac = |p: &str| -> Option<f32> {
            if p.ends_with('%') {
                p.trim_end_matches('%').parse::<f32>().ok().map(|v| (v / 100.0).clamp(0.0, 1.0))
            } else {
                None
            }
        };
        let x_kw = |p: &str| -> Option<f32> {
            match p {
                "left" => Some(0.0),
                "right" => Some(1.0),
                "center" => Some(0.5),
                _ => frac(p),
            }
        };
        let y_kw = |p: &str| -> Option<f32> {
            match p {
                "top" => Some(0.0),
                "bottom" => Some(1.0),
                "center" => Some(0.5),
                _ => frac(p),
            }
        };
        let px_val = |p: &str| -> Option<f32> {
            p.strip_suffix("px").and_then(|v| v.trim().parse::<f32>().ok())
        };
        let x_pos = |p: &str| -> Option<(Option<f32>, f32)> {
            if let Some(v) = px_val(p) {
                Some((Some(v), 0.0))
            } else {
                x_kw(p).map(|f| (None, f))
            }
        };
        let y_pos = |p: &str| -> Option<(Option<f32>, f32)> {
            if let Some(v) = px_val(p) {
                Some((Some(v), 0.0))
            } else {
                y_kw(p).map(|f| (None, f))
            }
        };
        match parts.len() {
            1 => {
                if let Some((px, f)) = x_pos(parts[0]) {
                    style.bg_pos_px_x = px;
                    style.bg_pos_x = f;
                }
                if let Some((px, f)) = y_pos(parts[0]) {
                    style.bg_pos_px_y = px;
                    style.bg_pos_y = f;
                }
            }
            2 => {
                if let Some((px, f)) = x_pos(parts[0]) {
                    style.bg_pos_px_x = px;
                    style.bg_pos_x = f;
                }
                if let Some((px, f)) = y_pos(parts[1]) {
                    style.bg_pos_px_y = px;
                    style.bg_pos_y = f;
                }
            }
            _ => {}
        }
    }
    if let Some(v) = get("background-repeat") {
        let low = v.trim().to_ascii_lowercase();
        style.bg_no_repeat = low == "no-repeat" || low.contains("no-repeat");
    }
    for prop in ["mask-image", "-webkit-mask-image"] {
        if let Some(v) = get(prop) {
            let low = v.trim().to_ascii_lowercase();
            if low != "none" {
                if let Some(url) = crate::css::extract_url(&v) {
                    style.mask_image = Some(url);
                }
            }
        }
    }
    if let Some(v) = get("background-image") {
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
    }
    if let Some(v) = get("text-align") {
        style.text_align = match v.trim().to_ascii_lowercase().as_str() {
            "center" | "-webkit-center" => TextAlign::Center,
            "right" | "end" | "-webkit-right" => TextAlign::Right,
            "justify" => TextAlign::Justify,
            _ => TextAlign::Left,
        };
    }
    if let Some(v) = get("text-decoration-line").or_else(|| get("text-decoration")) {
        let low = v.to_ascii_lowercase();
        style.text_decoration = TextDecoration {
            underline: low.contains("underline"),
            line_through: low.contains("line-through"),
            overline: low.contains("overline"),
        };
    }
    if let Some(v) = get("white-space") {
        style.white_space = match v.trim().to_ascii_lowercase().as_str() {
            "nowrap" => WhiteSpace::Nowrap,
            "pre" => WhiteSpace::Pre,
            "pre-wrap" | "break-spaces" => WhiteSpace::PreWrap,
            "pre-line" => WhiteSpace::PreLine,
            _ => WhiteSpace::Normal,
        };
    }
    if let Some(v) = get("text-transform") {
        style.text_transform = match v.trim().to_ascii_lowercase().as_str() {
            "uppercase" => TextTransform::Uppercase,
            "lowercase" => TextTransform::Lowercase,
            "capitalize" => TextTransform::Capitalize,
            _ => TextTransform::None,
        };
    }
    if let Some(v) = get("letter-spacing") {
        if let Some(l) = parse_length(&v, style.font_size, root_fs, viewport) {
            if let Length::Px(px) = l {
                style.letter_spacing = px;
            }
        }
    }
    if let Some(v) = get("word-spacing") {
        if let Some(l) = parse_length(&v, style.font_size, root_fs, viewport) {
            if let Length::Px(px) = l {
                style.word_spacing = px;
            }
        }
    }
    if let Some(v) = get("text-indent") {
        if let Some(l) = parse_length(&v, style.font_size, root_fs, viewport) {
            if let Length::Px(px) = l {
                style.text_indent = px;
            }
        }
    }

    for (i, side) in ["top", "right", "bottom", "left"].iter().enumerate() {
        if let Some(v) = get(&format!("margin-{}", side)) {
            if let Some(l) = parse_length(&v, style.font_size, root_fs, viewport) {
                style.margin[i] = l;
            }
        }
        if let Some(v) = get(&format!("padding-{}", side)) {
            if let Some(l) = parse_length(&v, style.font_size, root_fs, viewport) {
                style.padding[i] = match l {
                    Length::Auto => Length::Px(0.0),
                    other => other,
                };
            }
        }
        if let Some(v) = get(&format!("border-{}-width", side)) {
            let low = v.trim().to_ascii_lowercase();
            style.border_width[i] = match low.as_str() {
                "thin" => 1.0,
                "medium" => 3.0,
                "thick" => 5.0,
                _ => parse_length(&low, style.font_size, root_fs, viewport)
                    .and_then(|l| match l {
                        Length::Px(px) => Some(px),
                        Length::Percent(p) => Some(p * property_basis(viewport)),
                        _ => None,
                    })
                    .unwrap_or(style.border_width[i]),
            };
        }
        if let Some(v) = get(&format!("border-{}-style", side)) {
            style.border_style[i] = match v.trim().to_ascii_lowercase().as_str() {
                "solid" => BorderStyle::Solid,
                "dashed" => BorderStyle::Dashed,
                "dotted" => BorderStyle::Dotted,
                "double" => BorderStyle::Double,
                "none" | "hidden" => BorderStyle::None,
                _ => BorderStyle::Solid,
            };
        }
        if let Some(v) = get(&format!("border-{}-color", side)) {
            if let Some(c) = parse_color_value(&v, style.color) {
                style.border_color[i] = c;
            }
        }
        if style.border_style[i] == BorderStyle::None {
            // Border width without style does not render.
        }
    }
    if let Some(v) = get("border-radius") {
        let low = v.trim();
        let part = low.split_whitespace().next().unwrap_or("");
        let pct = part.ends_with('%');
        if let Some(l) = parse_length(part, style.font_size, root_fs, viewport) {
            style.border_radius = match l {
                Length::Px(px) => px.max(0.0),
                Length::Percent(p) => p * property_basis(viewport) * 0.5,
                Length::Auto => 0.0,
            };
            let _ = pct;
        }
    }
    for (prop, field) in [("width", 0), ("height", 1), ("min-width", 2), ("max-width", 3), ("min-height", 4), ("max-height", 5)] {
        if let Some(v) = get(prop) {
            let low = v.trim().to_ascii_lowercase();
            let len = if matches!(low.as_str(), "auto" | "max-content" | "min-content" | "fit-content" | "none" | "initial") {
                Length::Auto
            } else {
                parse_length(&low, style.font_size, root_fs, viewport).unwrap_or(Length::Auto)
            };
            match field {
                0 => style.width = len,
                1 => style.height = len,
                2 => style.min_width = len,
                3 => style.max_width = len,
                4 => style.min_height = len,
                _ => style.max_height = len,
            }
        }
    }
    if let Some(v) = get("box-sizing") {
        style.box_sizing_border = v.trim().eq_ignore_ascii_case("border-box");
    }
    if let Some(v) = get("overflow") {
        let low = v.trim().to_ascii_lowercase();
        style.overflow_hidden = matches!(low.as_str(), "hidden" | "auto" | "scroll" | "clip");
    }
    if let Some(v) = get("float") {
        style.float = match v.trim().to_ascii_lowercase().as_str() {
            "left" => Float::Left,
            "right" => Float::Right,
            _ => Float::None,
        };
    }
    if let Some(v) = get("list-style-type") {
        style.list_style_type = match v.trim().to_ascii_lowercase().as_str() {
            "disc" => ListStyleType::Disc,
            "circle" => ListStyleType::Circle,
            "square" => ListStyleType::Square,
            "decimal" => ListStyleType::Decimal,
            "decimal-leading-zero" => ListStyleType::DecimalLeadingZero,
            "lower-alpha" | "lower-latin" => ListStyleType::LowerAlpha,
            "upper-alpha" | "upper-latin" => ListStyleType::UpperAlpha,
            "lower-roman" => ListStyleType::LowerRoman,
            "upper-roman" => ListStyleType::UpperRoman,
            "none" => ListStyleType::None,
            _ => style.list_style_type,
        };
    }
    if let Some(v) = get("opacity") {
        if let Ok(n) = v.trim().parse::<f32>() {
            style.opacity = clamp(n, 0.0, 1.0);
        }
    }
    if let Some(v) = get("visibility") {
        style.visible = !v.trim().eq_ignore_ascii_case("hidden") && !v.trim().eq_ignore_ascii_case("collapse");
    }
    if let Some(v) = get("cursor") {
        style.cursor_pointer = v.to_ascii_lowercase().contains("pointer");
    }
    if let Some(v) = get("z-index") {
        if let Ok(n) = v.trim().parse::<i32>() {
            style.z_index = n;
        }
    }
    if let Some(v) = get("vertical-align") {
        let low = v.trim().to_ascii_lowercase();
        style.vertical_align = match low.as_str() {
            "top" | "text-top" => VerticalAlign::Top,
            "middle" => VerticalAlign::Middle,
            "bottom" | "text-bottom" => VerticalAlign::Bottom,
            "sub" => VerticalAlign::Sub,
            "super" => VerticalAlign::Super,
            "baseline" => VerticalAlign::Baseline,
            _ => parse_length(&low, style.font_size, root_fs, viewport)
                .and_then(|l| match l {
                    Length::Px(px) => Some(VerticalAlign::Px(px)),
                    Length::Percent(p) => Some(VerticalAlign::Px(p * style.font_size)),
                    _ => None,
                })
                .unwrap_or(VerticalAlign::Baseline),
        };
    }
    // Grid. The shorthand is applied first so longhands win.
    if let Some(v) = get("grid-template") {
        let value = v.trim().to_string();
        let (rows_part, cols_part) = split_grid_template(&value);
        let areas = parse_grid_areas(&rows_part);
        if !areas.is_empty() {
            style.grid_template_areas = areas;
        }
        let cleaned = strip_quoted_segments(&rows_part);
        if !cleaned.trim().is_empty() {
            style.grid_template_rows = cleaned.trim().to_string();
        }
        if !cols_part.trim().is_empty() {
            style.grid_template_columns = cols_part.trim().to_string();
        }
    }
    if let Some(v) = get("grid-template-columns") {
        style.grid_template_columns = v.trim().to_string();
    }
    if let Some(v) = get("grid-auto-rows") {
        style.grid_auto_rows = v.trim().to_string();
    }
    if let Some(v) = get("grid-template-rows") {
        style.grid_template_rows = v.trim().to_string();
    }
    if let Some(v) = get("grid-template-areas") {
        style.grid_template_areas = parse_grid_areas(&v);
    }
    if let Some(v) = get("grid-area") {
        style.grid_area = v.trim().trim_matches('"').trim_matches('\'').to_string();
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
    if let Some(v) = get("flex-direction") {
        style.flex_direction = match v.trim().to_ascii_lowercase().as_str() {
            "row-reverse" => FlexDirection::RowReverse,
            "column" => FlexDirection::Column,
            "column-reverse" => FlexDirection::ColumnReverse,
            _ => FlexDirection::Row,
        };
    }
    if let Some(v) = get("flex-wrap") {
        style.flex_wrap = v.contains("wrap");
    }
    if let Some(v) = get("justify-content") {
        style.justify_content = match v.trim().to_ascii_lowercase().as_str() {
            "center" => JustifyContent::Center,
            "flex-end" | "end" | "right" => JustifyContent::End,
            "space-between" => JustifyContent::SpaceBetween,
            "space-around" => JustifyContent::SpaceAround,
            "space-evenly" => JustifyContent::SpaceEvenly,
            _ => JustifyContent::Start,
        };
    }
    if let Some(v) = get("align-items") {
        style.align_items = parse_align(&v, style.align_items);
    }
    if let Some(v) = get("align-self") {
        style.align_self = parse_align(&v, style.align_self);
    }
    if let Some(v) = get("flex-grow") {
        if let Ok(n) = v.trim().parse::<f32>() {
            style.flex_grow = n.max(0.0);
        }
    }
    if let Some(v) = get("flex-shrink") {
        if let Ok(n) = v.trim().parse::<f32>() {
            style.flex_shrink = n.max(0.0);
        }
    }
    if let Some(v) = get("flex-basis") {
        let low = v.trim().to_ascii_lowercase();
        if low == "auto" || low == "content" {
            style.flex_basis = Length::Auto;
        } else if let Some(l) = parse_length(&low, style.font_size, root_fs, viewport) {
            style.flex_basis = l;
        }
    }
    if let Some(v) = get("column-gap") {
        if let Some(l) = parse_length(&v, style.font_size, root_fs, viewport) {
            if let Length::Px(px) = l {
                style.gap = px;
            }
        }
    }
    if let Some(v) = get("row-gap") {
        if let Some(l) = parse_length(&v, style.font_size, root_fs, viewport) {
            if let Length::Px(px) = l {
                style.row_gap = px;
            }
        }
    }
    if let Some(v) = get("top").or_else(|| get("bottom")).or_else(|| get("left")).or_else(|| get("right")) {
        let _ = v;
    }
    // Store offsets for positioned elements in custom map for layout.
    for prop in ["top", "right", "bottom", "left"] {
        if let Some(v) = get(prop) {
            let low = v.trim().to_ascii_lowercase();
            if low != "auto" {
                if let Some(l) = parse_length(&low, style.font_size, root_fs, viewport) {
                    let px = match l {
                        Length::Px(px) => px,
                        Length::Percent(p) => p * property_basis(viewport),
                        Length::Auto => continue,
                    };
                    style.custom.insert(format!("_inset-{}", prop), format!("{}", px));
                }
            }
        }
    }
}

fn parse_align(v: &str, default: AlignItems) -> AlignItems {
    match v.trim().to_ascii_lowercase().as_str() {
        "flex-start" | "start" | "self-start" | "left" => AlignItems::Start,
        "center" => AlignItems::Center,
        "flex-end" | "end" | "self-end" | "right" => AlignItems::End,
        "baseline" => AlignItems::Baseline,
        "stretch" => AlignItems::Stretch,
        _ => default,
    }
}

fn apply_html_attributes(doc: &Document, node: NodeId, style: &mut ComputedStyle, viewport: (f32, f32)) {
    let el = match doc.element(node) {
        Some(e) => e,
        None => return,
    };
    let tag = el.tag.as_str();
    if style.background_color.a == 0 {
        if let Some(bg) = el.attr("bgcolor") {
            if let Some(c) = parse_color(bg) {
                style.background_color = c;
            }
        }
    }
    if let Some(v) = el.attr("align") {
        style.text_align = match v.to_ascii_lowercase().as_str() {
            "center" => TextAlign::Center,
            "right" => TextAlign::Right,
            _ => TextAlign::Left,
        };
    }
    if style.width.is_auto() {
        if let Some(w) = el.attr("width") {
            if let Some(l) = parse_html_dimension(w) {
                style.width = l;
            }
        }
    }
    if style.height.is_auto() {
        if let Some(h) = el.attr("height") {
            if let Some(l) = parse_html_dimension(h) {
                style.height = l;
            }
        }
    }
    // Old <table border> attribute: draw cell borders.
    if tag == "td" || tag == "th" {
        let mut cur = doc.nodes[node].parent;
        while let Some(p) = cur {
            if doc.tag(p) == Some("table") {
                if let Some(b) = doc.attr(p, "border") {
                    if b != "0" && !b.is_empty() {
                        for i in 0..4 {
                            if style.border_style[i] == BorderStyle::None {
                                style.border_style[i] = BorderStyle::Solid;
                                style.border_width[i] = 1.0;
                                style.border_color[i] = Color::rgb(128, 128, 128);
                            }
                        }
                    }
                }
                break;
            }
            cur = doc.nodes[p].parent;
        }
    }
    if tag == "table" {
        if let Some(b) = el.attr("border") {
            if b != "0" && !b.is_empty() {
                if style.border_style[0] == BorderStyle::None {
                    style.border_style = [BorderStyle::Solid; 4];
                    style.border_width = [1.0; 4];
                    style.border_color = [Color::rgb(128, 128, 128); 4];
                }
            }
        }
    }
    let _ = viewport;
}

pub fn parse_linear_gradient(value: &str) -> Option<Gradient> {
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

pub fn parse_grid_areas(value: &str) -> Vec<Vec<String>> {
    let mut rows: Vec<Vec<String>> = Vec::new();
    let chars: Vec<char> = value.chars().collect();
    let mut i = 0;
    while i < chars.len() {
        if chars[i] == '"' || chars[i] == '\'' {
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
            '"' | '\'' => quote = Some(c),
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
            '"' | '\'' => quote = Some(c),
            _ => out.push(c),
        }
    }
    out
}

fn parse_html_dimension(value: &str) -> Option<Length> {
    let v = value.trim();
    if v.is_empty() {
        return None;
    }
    if let Some(p) = v.strip_suffix('%') {
        return p.trim().parse::<f32>().ok().map(|n| Length::Percent(n / 100.0));
    }
    if let Ok(n) = v.parse::<f32>() {
        return Some(Length::Px(n));
    }
    parse_length(v, 16.0, 16.0, (1000.0, 800.0))
}

/// Best-effort text transform application used by layout.
pub fn transform_text(text: &str, transform: TextTransform) -> String {
    match transform {
        TextTransform::None => text.to_string(),
        TextTransform::Uppercase => text.to_uppercase(),
        TextTransform::Lowercase => text.to_lowercase(),
        TextTransform::Capitalize => {
            let mut out = String::with_capacity(text.len());
            let mut prev_space = true;
            for c in text.chars() {
                if prev_space {
                    for u in c.to_uppercase() {
                        out.push(u);
                    }
                } else {
                    out.push(c);
                }
                prev_space = c.is_whitespace();
            }
            out
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn cascade_ua_and_author() {
        let doc = crate::html::parse_document("<html><head><style>p { color: red; }</style></head><body><p>Hi</p></body></html>");
        let stylesheet = crate::css::parse_stylesheet("p { color: rgb(255, 0, 0); }");
        let styles = compute_styles(&doc, &stylesheet, (1000.0, 800.0), None, None, None);
        let body = doc.walk(doc.root).into_iter().find(|&n| doc.tag(n) == Some("body")).unwrap();
        let p = doc.walk(doc.root).into_iter().find(|&n| doc.tag(n) == Some("p")).unwrap();
        println!("body margin left {:?} display {:?}", styles[body].margin[3], styles[body].display);
        println!("p color {:?} display {:?} margin {:?}", styles[p].color, styles[p].display, styles[p].margin);
        assert_eq!(styles[body].margin[3], Length::Px(8.0));
        assert_eq!(styles[p].color, Color::rgb(255,0,0));
        assert_eq!(styles[p].display, Display::Block);
    }
}

#[cfg(test)]
mod ua_debug {
    use super::*;
    #[test]
    fn inspect_ua() {
        let ua = ua_sheet();
        println!("UA rules: {}", ua.rules.len());
        for r in &ua.rules {
            for sel in &r.selectors {
                let desc: Vec<String> = sel.compounds.iter().map(|c| format!("{:?}", c.simples)).collect();
                if desc.iter().any(|d| d.contains("\"body\"")) {
                    println!("selector {:?} decls {:?}", desc, r.decls.iter().map(|d| (&d.name, &d.value)).collect::<Vec<_>>());
                }
            }
        }
    }
}

#[cfg(test)]
mod grid_tests {
    use super::*;
    #[test]
    fn grid_css_applies() {
        let doc = crate::html::parse_document("<html><body><div class='wrap'><div class='a'>A</div></div></body></html>");
        let css = crate::css::parse_stylesheet(".wrap { display: grid; grid-template-columns: 200px 1fr; grid-template-areas: 'left right'; } .a { grid-area: right; }");
        println!("rules {} sel0 {:?}", css.rules.len(), css.rules.get(0).map(|r| r.selectors.len()));
        let styles = compute_styles(&doc, &css, (1000.0, 800.0), None, None, None);
        for n in doc.walk(doc.root) {
            if let Some(cls) = doc.attr(n, "class") {
                println!("{} display={:?} cols={:?} areas={:?}", cls, styles[n].display, styles[n].grid_template_columns, styles[n].grid_template_areas);
            }
        }
        let wrap = doc.walk(doc.root).into_iter().find(|&n| doc.attr(n,"class")==Some("wrap")).unwrap();
        assert_eq!(styles[wrap].display, Display::Grid);
    }
}

#[cfg(test)]
mod grid_tests2 {
    use super::*;
    #[test]
    fn grid_template_shorthand() {
        let css = crate::css::parse_stylesheet(".mw-page-container-inner{display:grid;column-gap:24px;grid-template:min-content 1fr min-content / 12.25rem minmax(0,1fr);grid-template-areas:'siteNotice siteNotice' 'columnStart pageContent' 'footer footer'}");
        for r in &css.rules {
            println!("rule decls {:?}", r.decls.iter().map(|d| (&d.name, &d.value)).collect::<Vec<_>>());
        }
        let doc = crate::html::parse_document("<html><body><div class='mw-page-container-inner'>x</div></body></html>");
        let styles = compute_styles(&doc, &css, (1200.0, 800.0), None, None, None);
        let w = doc.walk(doc.root).into_iter().find(|&n| doc.attr(n,"class")==Some("mw-page-container-inner")).unwrap();
        println!("disp={:?} areas={:?} cols={:?} rows={:?}", styles[w].display, styles[w].grid_template_areas, styles[w].grid_template_columns, styles[w].grid_template_rows);
        assert!(!styles[w].grid_template_areas.is_empty());
    }
}

#[cfg(test)]
mod areas_direct {
    use super::*;
    #[test]
    fn direct() {
        println!("A {:?}", parse_grid_areas("'a b' 'c d'"));
        println!("B {:?}", parse_grid_areas("'siteNotice siteNotice' 'columnStart pageContent' 'footer footer'"));
        let s = "'siteNotice siteNotice' 'columnStart pageContent' 'footer footer'";
        for c in s.chars().take(6) { println!("char {:?}", c); }
    }
}

#[cfg(test)]
mod float_test {
    use super::*;
    #[test]
    fn float_attr() {
        let doc = crate::html::parse_document("<html><body><figure class='mw-halign-right' typeof='mw:File/Thumb'><img src='x'></figure></body></html>");
        let css = crate::css::parse_stylesheet("figure[typeof~='mw:File/Thumb']{display:table;float:right} .mw-halign-right{float:right}");
        println!("rules {}", css.rules.len());
        let styles = compute_styles(&doc, &css, (1000.0,800.0), None,None,None);
        let f = doc.walk(doc.root).into_iter().find(|&n| doc.tag(n)==Some("figure")).unwrap();
        println!("display={:?} float={:?}", styles[f].display, styles[f].float);
        assert_eq!(styles[f].float, Float::Right);
    }
}
