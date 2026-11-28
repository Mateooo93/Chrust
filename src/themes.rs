//! Color themes, shared by the GPUI interface and the built-in pages.

use gpui_kit::component::{Theme, ThemeMode};
use gpui_kit::{App, Hsla, Rgba, WindowAppearance};

#[derive(Clone, Copy)]
pub struct Palette {
    pub id: &'static str,
    pub name: &'static str,
    pub dark: bool,
    /// Page and active-tab background.
    pub bg: u32,
    /// Title bar and tab strip.
    pub surface: u32,
    /// Inputs, cards, hovered rows.
    pub elevated: u32,
    pub border: u32,
    pub fg: u32,
    pub muted: u32,
    pub accent: u32,
    /// Second color for gradients on the new tab page.
    pub accent2: u32,
}

const fn p(
    id: &'static str,
    name: &'static str,
    dark: bool,
    [bg, surface, elevated, border, fg, muted, accent, accent2]: [u32; 8],
) -> Palette {
    Palette { id, name, dark, bg, surface, elevated, border, fg, muted, accent, accent2 }
}

pub const PALETTES: &[Palette] = &[
    p("chrustport", "ChrustPort", true, [0x10131f, 0x0b0d17, 0x1a1f33, 0x262c45, 0xe6e8f2, 0x8b91a8, 0x8b7bff, 0x2dd4bf]),
    p("midnight", "Midnight", true, [0x0c0c0f, 0x060608, 0x17171c, 0x24242b, 0xeaeaea, 0x8a8a94, 0x22d3ee, 0xa78bfa]),
    p("tokyo-night", "Tokyo Night", true, [0x1a1b26, 0x16161e, 0x24283b, 0x2f3549, 0xc0caf5, 0x7a82a8, 0x7aa2f7, 0xbb9af7]),
    p("catppuccin", "Catppuccin Mocha", true, [0x1e1e2e, 0x181825, 0x2a2b3d, 0x393a50, 0xcdd6f4, 0x9399b2, 0xcba6f7, 0xf5c2e7]),
    p("dracula", "Dracula", true, [0x282a36, 0x21222c, 0x343746, 0x44475a, 0xf8f8f2, 0x9ea8c7, 0xbd93f9, 0xff79c6]),
    p("nord", "Nord", true, [0x2e3440, 0x272c36, 0x3b4252, 0x434c5e, 0xeceff4, 0x9aa5b8, 0x88c0d0, 0xb48ead]),
    p("gruvbox", "Gruvbox", true, [0x282828, 0x1d2021, 0x32302f, 0x45403d, 0xebdbb2, 0xa89984, 0xfabd2f, 0xfe8019]),
    p("light", "Daylight", false, [0xffffff, 0xf1f3f7, 0xf6f7f9, 0xdfe3ea, 0x1d2128, 0x667085, 0x5b5bf0, 0x0ea5e9]),
    p("github-light", "GitHub Light", false, [0xffffff, 0xf6f8fa, 0xf6f8fa, 0xd0d7de, 0x1f2328, 0x656d76, 0x0969da, 0x8250df]),
    p("paper", "Paper", false, [0xfbfaf7, 0xf0ede6, 0xf5f3ee, 0xe2ddd2, 0x2b2722, 0x7a7266, 0xd9480f, 0xc2185b]),
];

/// The "system" choice picks one of these from the Windows light/dark setting.
pub const SYSTEM_DARK: &str = "chrustport";
pub const SYSTEM_LIGHT: &str = "light";

pub fn palette(id: &str, appearance: WindowAppearance) -> &'static Palette {
    let id = if id == "system" {
        match appearance {
            WindowAppearance::Dark | WindowAppearance::VibrantDark => SYSTEM_DARK,
            _ => SYSTEM_LIGHT,
        }
    } else {
        id
    };
    PALETTES.iter().find(|p| p.id == id).unwrap_or(&PALETTES[0])
}

fn rgba(hex: u32, a: f32) -> Rgba {
    Rgba {
        r: ((hex >> 16) & 0xff) as f32 / 255.0,
        g: ((hex >> 8) & 0xff) as f32 / 255.0,
        b: (hex & 0xff) as f32 / 255.0,
        a,
    }
}

fn hsla(hex: u32) -> Hsla {
    rgba(hex, 1.0).into()
}

/// `a` blended toward `b` by `t` (0..1).
fn mix(a: u32, b: u32, t: f32) -> u32 {
    let ch = |shift: u32| {
        let x = ((a >> shift) & 0xff) as f32;
        let y = ((b >> shift) & 0xff) as f32;
        ((x + (y - x) * t).round() as u32) << shift
    };
    ch(16) | ch(8) | ch(0)
}

/// Readable text color on top of `bg`.
fn on(bg: u32) -> u32 {
    let c = rgba(bg, 1.0);
    if 0.299 * c.r + 0.587 * c.g + 0.114 * c.b > 0.6 { 0x111111 } else { 0xffffff }
}

pub fn apply(p: &Palette, cx: &mut App) {
    Theme::change(if p.dark { ThemeMode::Dark } else { ThemeMode::Light }, None, cx);
    let hover = mix(p.elevated, p.fg, 0.06);
    let active = mix(p.elevated, p.fg, 0.12);
    Theme::update(cx, |t| {
        t.radius = gpui_kit::px(8.);
        t.radius_lg = gpui_kit::px(12.);
        t.shadow = true;

        t.background = hsla(p.bg);
        t.foreground = hsla(p.fg);
        t.border = hsla(p.border);
        t.input = hsla(p.border);
        t.ring = hsla(p.accent);
        t.caret = hsla(p.accent);
        t.selection = rgba(p.accent, 0.35).into();
        t.muted = hsla(p.elevated);
        t.muted_foreground = hsla(p.muted);
        t.link = hsla(p.accent);
        t.link_hover = hsla(mix(p.accent, p.fg, 0.25));
        t.link_active = hsla(p.accent);

        t.primary = hsla(p.accent);
        t.primary_hover = hsla(mix(p.accent, p.fg, 0.12));
        t.primary_active = hsla(mix(p.accent, p.bg, 0.15));
        t.primary_foreground = hsla(on(p.accent));
        t.button_primary = t.primary;
        t.button_primary_hover = t.primary_hover;
        t.button_primary_active = t.primary_active;
        t.button_primary_foreground = t.primary_foreground;

        t.secondary = hsla(p.elevated);
        t.secondary_hover = hsla(hover);
        t.secondary_active = hsla(active);
        t.secondary_foreground = hsla(p.fg);
        t.button = t.secondary;
        t.button_hover = t.secondary_hover;
        t.button_active = t.secondary_active;
        t.button_foreground = t.secondary_foreground;
        t.button_secondary = t.secondary;
        t.button_secondary_hover = t.secondary_hover;
        t.button_secondary_active = t.secondary_active;
        t.button_secondary_foreground = t.secondary_foreground;

        t.accent = hsla(hover);
        t.accent_foreground = hsla(p.fg);
        t.list_hover = hsla(hover);
        t.list_active = rgba(p.accent, 0.18).into();
        t.list_active_border = hsla(p.accent);

        t.popover = hsla(p.elevated);
        t.popover_foreground = hsla(p.fg);
        t.title_bar = hsla(p.surface);
        t.title_bar_border = hsla(p.surface);
        t.tab_bar = hsla(p.surface);
        t.tab = hsla(p.surface);
        t.tab_active = hsla(p.bg);
        t.tab_foreground = hsla(p.muted);
        t.tab_active_foreground = hsla(p.fg);
        t.sidebar = hsla(p.surface);
        t.sidebar_border = hsla(p.border);
        t.sidebar_foreground = hsla(p.fg);
        t.scrollbar = rgba(0, 0.0).into();
        t.scrollbar_thumb = rgba(p.muted, 0.35).into();
        t.scrollbar_thumb_hover = rgba(p.muted, 0.6).into();
        t.window_border = hsla(p.border);
    });
}

fn css_hex(hex: u32) -> String {
    format!("#{hex:06x}")
}

/// Page background WebView2 shows before a page paints, so new tabs do not
/// flash white in dark themes.
pub fn css_bg(p: &Palette) -> (u8, u8, u8, u8) {
    (((p.bg >> 16) & 0xff) as u8, ((p.bg >> 8) & 0xff) as u8, (p.bg & 0xff) as u8, 255)
}

/// CSS custom properties for the built-in pages.
pub fn css_vars(p: &Palette) -> String {
    format!(
        "--bg:{};--surface:{};--elevated:{};--border:{};--fg:{};--muted:{};--accent:{};--accent2:{};--on-accent:{};color-scheme:{};",
        css_hex(p.bg),
        css_hex(p.surface),
        css_hex(p.elevated),
        css_hex(p.border),
        css_hex(p.fg),
        css_hex(p.muted),
        css_hex(p.accent),
        css_hex(p.accent2),
        css_hex(on(p.accent)),
        if p.dark { "dark" } else { "light" },
    )
}

/// All palettes as JSON, so the settings page can preview and switch live.
pub fn palettes_json() -> String {
    let items: Vec<serde_json::Value> = PALETTES
        .iter()
        .map(|p| {
            serde_json::json!({
                "id": p.id,
                "name": p.name,
                "dark": p.dark,
                "vars": css_vars(p),
                "swatch": [css_hex(p.bg), css_hex(p.elevated), css_hex(p.accent), css_hex(p.accent2)],
            })
        })
        .collect();
    serde_json::Value::Array(items).to_string()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ids_are_unique_and_system_resolves() {
        let mut ids: Vec<_> = PALETTES.iter().map(|p| p.id).collect();
        ids.sort();
        ids.dedup();
        assert_eq!(ids.len(), PALETTES.len());
        assert_eq!(palette("system", WindowAppearance::Dark).id, SYSTEM_DARK);
        assert_eq!(palette("system", WindowAppearance::Light).id, SYSTEM_LIGHT);
        assert_eq!(palette("nope", WindowAppearance::Light).id, "chrustport");
    }

    #[test]
    fn mixing_and_contrast() {
        assert_eq!(mix(0x000000, 0xffffff, 0.5), 0x808080);
        assert_eq!(on(0xffffff), 0x111111);
        assert_eq!(on(0x10131f), 0xffffff);
    }
}
