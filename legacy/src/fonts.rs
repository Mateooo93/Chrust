#![allow(dead_code)]
//! Font management: finds system fonts, measures runs with per-character
//! fallback, and rasterizes antialiased glyphs.

use std::collections::HashMap;
use std::path::PathBuf;
use std::rc::Rc;

use fontdue::{Font, FontSettings};

pub struct Face {
    pub font: Font,
    pub family: String,
    pub bold: bool,
    pub italic: bool,
}

#[derive(Clone)]
pub struct Glyph {
    pub width: usize,
    pub height: usize,
    pub left: f32,
    pub top: f32,
    pub advance: f32,
    pub bitmap: Vec<u8>,
}

pub struct FontStore {
    faces: Vec<Face>,
    family_cache: HashMap<(String, bool, bool), usize>,
    glyph_cache: HashMap<(usize, char, u32), Rc<Glyph>>,
    char_face_cache: HashMap<(usize, char), usize>,
    font_dirs: Vec<PathBuf>,
}

fn norm_family(family: &str) -> String {
    let mut f = family
        .trim()
        .trim_matches('"')
        .trim_matches('\'')
        .to_ascii_lowercase();
    match f.as_str() {
        "sans-serif" | "system-ui" | "-apple-system" | "ui-sans-serif" | "segoe ui"
        | "helvetica neue" | "helvetica" | "arial" | "sans" => {
            f = "segoe ui".to_string();
        }
        "serif" | "ui-serif" | "times" | "times new roman" | "georgia" | "garamond" => {
            f = "times new roman".to_string();
        }
        "monospace" | "ui-monospace" | "consolas" | "courier new" | "courier" | "monaco" => {
            f = "consolas".to_string();
        }
        "cursive" | "comic sans ms" => {
            f = "comic sans ms".to_string();
        }
        "fantasy" | "impact" => {
            f = "impact".to_string();
        }
        _ => {}
    }
    f
}

fn candidates(family: &str, bold: bool, italic: bool) -> Vec<&'static str> {
    let files: &[&str] = match family {
        "segoe ui" => match (bold, italic) {
            (false, false) => &["segoeui.ttf"],
            (true, false) => &["segoeuib.ttf"],
            (false, true) => &["segoeuii.ttf"],
            (true, true) => &["segoeuiz.ttf"],
        },
        "arial" => match (bold, italic) {
            (false, false) => &["arial.ttf"],
            (true, false) => &["arialbd.ttf"],
            (false, true) => &["ariali.ttf"],
            (true, true) => &["arialbi.ttf"],
        },
        "times new roman" => match (bold, italic) {
            (false, false) => &["times.ttf"],
            (true, false) => &["timesbd.ttf"],
            (false, true) => &["timesi.ttf"],
            (true, true) => &["timesbi.ttf"],
        },
        "georgia" => match (bold, italic) {
            (false, false) => &["georgia.ttf"],
            (true, false) => &["georgiab.ttf"],
            (false, true) => &["georgiai.ttf"],
            (true, true) => &["georgiaz.ttf"],
        },
        "consolas" => match (bold, italic) {
            (false, false) => &["consola.ttf"],
            (true, false) => &["consolab.ttf"],
            (false, true) => &["consolai.ttf"],
            (true, true) => &["consolaz.ttf"],
        },
        "courier new" => match (bold, italic) {
            (false, false) => &["cour.ttf"],
            (true, false) => &["courbd.ttf"],
            (false, true) => &["couri.ttf"],
            (true, true) => &["courbi.ttf"],
        },
        "tahoma" => match (bold, italic) {
            (false, false) => &["tahoma.ttf"],
            (true, false) => &["tahomabd.ttf"],
            (false, true) => &["tahoma.ttf", "tahomabd.ttf"],
            (true, true) => &["tahomabd.ttf"],
        },
        "verdana" => match (bold, italic) {
            (false, false) => &["verdana.ttf"],
            (true, false) => &["verdanab.ttf"],
            (false, true) => &["verdanai.ttf"],
            (true, true) => &["verdanaz.ttf"],
        },
        "comic sans ms" => match (bold, italic) {
            (false, false) => &["comic.ttf"],
            (true, false) => &["comicbd.ttf"],
            (false, true) => &["comici.ttf"],
            (true, true) => &["comicz.ttf"],
        },
        "calibri" => match (bold, italic) {
            (false, false) => &["calibri.ttf"],
            (true, false) => &["calibrib.ttf"],
            (false, true) => &["calibrii.ttf"],
            (true, true) => &["calibriz.ttf"],
        },
        "cambria" => match (bold, italic) {
            (false, false) => &["cambria.ttf"],
            (true, false) => &["cambriab.ttf"],
            (false, true) => &["cambriai.ttf"],
            (true, true) => &["cambriaz.ttf"],
        },
        "trebuchet ms" => match (bold, italic) {
            (false, false) => &["trebuc.ttf"],
            (true, false) => &["trebucbd.ttf"],
            (false, true) => &["trebucit.ttf"],
            (true, true) => &["trebucbi.ttf"],
        },
        "segoe ui symbol" => &["seguisym.ttf"],
        "segoe ui emoji" => &["seguiemj.ttf"],
        "arial black" => &["ariblk.ttf"],
        "impact" => &["impact.ttf"],
        "lucida console" => &["lucon.ttf"],
        "lucida sans unicode" => &["l_10646.ttf"],
        "palatino linotype" => match (bold, italic) {
            (false, false) => &["pala.ttf"],
            (true, false) => &["palab.ttf"],
            (false, true) => &["palai.ttf"],
            (true, true) => &["palabi.ttf"],
        },
        "candara" => match (bold, italic) {
            (false, false) => &["Candara.ttf"],
            (true, false) => &["Candarab.ttf"],
            (false, true) => &["Candarai.ttf"],
            (true, true) => &["Candaraz.ttf"],
        },
        "constantia" => match (bold, italic) {
            (false, false) => &["constan.ttf"],
            (true, false) => &["constanb.ttf"],
            (false, true) => &["constani.ttf"],
            (true, true) => &["constanz.ttf"],
        },
        "corbel" => match (bold, italic) {
            (false, false) => &["corbel.ttf"],
            (true, false) => &["corbelb.ttf"],
            (false, true) => &["corbeli.ttf"],
            (true, true) => &["corbelz.ttf"],
        },
        "dejavu sans" => match (bold, italic) {
            (false, false) => &["DejaVuSans.ttf"],
            (true, false) => &["DejaVuSans-Bold.ttf"],
            (false, true) => &["DejaVuSans-Oblique.ttf"],
            (true, true) => &["DejaVuSans-BoldOblique.ttf"],
        },
        "liberation sans" => match (bold, italic) {
            (false, false) => &["LiberationSans-Regular.ttf"],
            (true, false) => &["LiberationSans-Bold.ttf"],
            (false, true) => &["LiberationSans-Italic.ttf"],
            (true, true) => &["LiberationSans-BoldItalic.ttf"],
        },
        _ => &[],
    };
    files.to_vec()
}

impl FontStore {
    pub fn new() -> FontStore {
        let mut dirs = Vec::new();
        if let Ok(root) = std::env::var("SystemRoot") {
            dirs.push(PathBuf::from(root).join("Fonts"));
        }
        dirs.push(PathBuf::from("C:\\Windows\\Fonts"));
        dirs.push(PathBuf::from("/usr/share/fonts"));
        dirs.push(PathBuf::from("/usr/local/share/fonts"));
        dirs.push(PathBuf::from("/System/Library/Fonts"));
        dirs.push(PathBuf::from("/Library/Fonts"));
        let mut store = FontStore {
            faces: Vec::new(),
            family_cache: HashMap::new(),
            glyph_cache: HashMap::new(),
            char_face_cache: HashMap::new(),
            font_dirs: dirs,
        };
        // Make sure the default family is available.
        store.face_for("segoe ui", false, false);
        store
    }

    fn find_font_file(&self, name: &str) -> Option<PathBuf> {
        let lower = name.to_ascii_lowercase();
        for dir in &self.font_dirs {
            let direct = dir.join(name);
            if direct.is_file() {
                return Some(direct);
            }
            // Case-insensitive lookup in Windows fonts.
            if let Ok(entries) = std::fs::read_dir(dir) {
                for e in entries.flatten() {
                    if e.file_name().to_string_lossy().to_ascii_lowercase() == lower {
                        return Some(e.path());
                    }
                }
            }
        }
        None
    }

    fn load_family_face(&mut self, family: &str, bold: bool, italic: bool) -> Option<usize> {
        let mut fallback_styles = Vec::new();
        for (b, i) in [(bold, italic), (bold, false), (false, italic), (false, false)] {
            if !fallback_styles.contains(&(b, i)) {
                fallback_styles.push((b, i));
            }
        }
        let mut files = candidates(family, bold, italic);
        // Extra compatibility fallbacks for common Windows fonts.
        if files.is_empty() {
            let extras = match family {
                "georgia" => vec!["georgia.ttf"],
                "calibri" => vec!["calibri.ttf"],
                "cambria" => vec!["cambria.ttf"],
                "trebuchet ms" => vec!["trebuc.ttf"],
                "verdana" => vec!["verdana.ttf"],
                _ => Vec::new(),
            };
            files = extras;
        }
        for f in files {
            if let Some(path) = self.find_font_file(f) {
                if let Ok(bytes) = std::fs::read(&path) {
                    if let Ok(font) = Font::from_bytes(bytes, FontSettings::default()) {
                        let idx = self.faces.len();
                        self.faces.push(Face {
                            font,
                            family: family.to_string(),
                            bold,
                            italic,
                        });
                        return Some(idx);
                    }
                }
            }
        }
        // Try the other styles of the same family.
        let mut tried = Vec::new();
        for (b, i) in fallback_styles {
            let files = candidates(family, b, i);
            for f in files {
                if tried.contains(&f) {
                    continue;
                }
                tried.push(f);
                if let Some(path) = self.find_font_file(f) {
                    if let Ok(bytes) = std::fs::read(&path) {
                        if let Ok(font) = Font::from_bytes(bytes, FontSettings::default()) {
                            let idx = self.faces.len();
                            self.faces.push(Face {
                                font,
                                family: family.to_string(),
                                bold,
                                italic,
                            });
                            return Some(idx);
                        }
                    }
                }
            }
        }
        None
    }

    pub fn face_for(&mut self, family: &str, bold: bool, italic: bool) -> usize {
        let key = (norm_family(family), bold, italic);
        if let Some(&idx) = self.family_cache.get(&key) {
            return idx;
        }
        let idx = self
            .load_family_face(&key.0, bold, italic)
            .or_else(|| {
                if key.0 != "segoe ui" {
                    self.load_family_face("segoe ui", bold, italic)
                } else {
                    None
                }
            })
            .or_else(|| self.load_family_face("arial", bold, italic))
            .or_else(|| self.load_family_face("dejavu sans", bold, italic))
            .or_else(|| {
                if !self.faces.is_empty() {
                    Some(0)
                } else {
                    None
                }
            });
        let idx = match idx {
            Some(i) => i,
            None => {
                // Absolute last resort: try every file in the font directory.
                self.load_any_font().unwrap_or(usize::MAX)
            }
        };
        self.family_cache.insert(key, idx);
        idx
    }

    fn load_any_font(&mut self) -> Option<usize> {
        for dir in self.font_dirs.clone() {
            if let Ok(entries) = std::fs::read_dir(&dir) {
                for e in entries.flatten() {
                    let p = e.path();
                    let ext = p
                        .extension()
                        .map(|e| e.to_string_lossy().to_ascii_lowercase())
                        .unwrap_or_default();
                    if (ext == "ttf" || ext == "otf") && !p.to_string_lossy().to_ascii_lowercase().contains("emoji") {
                        if let Ok(bytes) = std::fs::read(&p) {
                            if let Ok(font) = Font::from_bytes(bytes, FontSettings::default()) {
                                let idx = self.faces.len();
                                self.faces.push(Face {
                                    font,
                                    family: "fallback".into(),
                                    bold: false,
                                    italic: false,
                                });
                                return Some(idx);
                            }
                        }
                    }
                }
            }
        }
        None
    }

    pub fn resolve_char_face(&mut self, primary: usize, ch: char) -> usize {
        let key = (primary, ch);
        if let Some(&idx) = self.char_face_cache.get(&key) {
            return idx;
        }
        let mut result = primary;
        if primary != usize::MAX {
            let has = self.faces.get(primary).map(|f| f.font.lookup_glyph_index(ch) != 0).unwrap_or(false);
            if !has {
                for fam in [
                    "segoe ui symbol",
                    "segoe ui emoji",
                    "segoe ui",
                    "arial",
                    "times new roman",
                    "consolas",
                    "cambria math",
                ] {
                    let fidx = self.face_for(fam, false, false);
                    if fidx != usize::MAX
                        && self
                            .faces
                            .get(fidx)
                            .map(|f| f.font.lookup_glyph_index(ch) != 0)
                            .unwrap_or(false)
                    {
                        result = fidx;
                        break;
                    }
                }
            }
        }
        self.char_face_cache.insert(key, result);
        result
    }

    fn face_glyph_advance(&self, face: usize, ch: char, size: f32) -> f32 {
        if let Some(f) = self.faces.get(face) {
            let m = f.font.metrics(ch, size);
            m.advance_width
        } else {
            size * 0.5
        }
    }

    pub fn measure(
        &mut self,
        text: &str,
        family: &str,
        bold: bool,
        italic: bool,
        size: f32,
        letter_spacing: f32,
        word_spacing: f32,
    ) -> f32 {
        let primary = self.face_for(family, bold, italic);
        let mut total = 0.0;
        for ch in text.chars() {
            let face = self.resolve_char_face(primary, ch);
            total += self.face_glyph_advance(face, ch, size);
            total += letter_spacing;
            if ch == ' ' {
                total += word_spacing;
            }
        }
        total
    }

    pub fn measure_style(&mut self, text: &str, style: &crate::style::ComputedStyle) -> f32 {
        self.measure(
            text,
            &style.font_family,
            style.font_weight >= 600,
            style.font_italic,
            style.font_size,
            style.letter_spacing,
            style.word_spacing,
        )
    }

    pub fn char_advance(&mut self, ch: char, style: &crate::style::ComputedStyle) -> f32 {
        let primary = self.face_for(&style.font_family, style.font_weight >= 600, style.font_italic);
        let face = self.resolve_char_face(primary, ch);
        let mut adv = self.face_glyph_advance(face, ch, style.font_size);
        adv += style.letter_spacing;
        if ch == ' ' {
            adv += style.word_spacing;
        }
        adv
    }

    pub fn line_metrics(&mut self, style: &crate::style::ComputedStyle) -> (f32, f32) {
        let primary = self.face_for(&style.font_family, style.font_weight >= 600, style.font_italic);
        if let Some(f) = self.faces.get(primary) {
            if let Some(m) = f.font.horizontal_line_metrics(style.font_size) {
                return (m.ascent, m.descent.abs());
            }
        }
        (style.font_size * 0.8, style.font_size * 0.2)
    }

    pub fn ascent(&mut self, family: &str, bold: bool, italic: bool, size: f32) -> f32 {
        let idx = self.face_for(family, bold, italic);
        if let Some(f) = self.faces.get(idx) {
            if let Some(m) = f.font.horizontal_line_metrics(size) {
                return m.ascent;
            }
        }
        size * 0.8
    }

    pub fn glyph(&mut self, ch: char, style: &crate::style::ComputedStyle) -> Rc<Glyph> {
        let primary = self.face_for(&style.font_family, style.font_weight >= 600, style.font_italic);
        let face = self.resolve_char_face(primary, ch);
        let size_q = ((style.font_size * 4.0).round() as u32).max(1);
        let size = size_q as f32 / 4.0;
        let key = (face, ch, size_q);
        if let Some(g) = self.glyph_cache.get(&key) {
            return g.clone();
        }
        let glyph = if let Some(f) = self.faces.get(face) {
            let (m, bitmap) = f.font.rasterize(ch, size);
            Glyph {
                width: m.width,
                height: m.height,
                left: m.xmin as f32,
                top: (m.ymin + m.height as i32) as f32,
                advance: m.advance_width,
                bitmap,
            }
        } else {
            Glyph {
                width: 0,
                height: 0,
                left: 0.0,
                top: 0.0,
                advance: size * 0.5,
                bitmap: Vec::new(),
            }
        };
        let glyph = Rc::new(glyph);
        self.glyph_cache.insert(key, glyph.clone());
        glyph
    }

    pub fn glyph_advance_at(&mut self, ch: char, style: &crate::style::ComputedStyle) -> f32 {
        self.char_advance(ch, style)
    }

    pub fn has_font(&self) -> bool {
        !self.faces.is_empty()
    }
}
