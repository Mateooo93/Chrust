#![allow(dead_code)]
//! Native window, browser chrome, input handling and navigation.

use std::num::NonZeroU32;
use std::sync::Arc;
use std::thread;
use std::time::{Duration, Instant};

use winit::application::ApplicationHandler;
use winit::dpi::LogicalSize;
use winit::event::{ElementState, MouseButton, MouseScrollDelta, WindowEvent};
use winit::event_loop::{ActiveEventLoop, ControlFlow, EventLoop, EventLoopProxy};
use winit::keyboard::{Key, ModifiersState, NamedKey};
use winit::window::{CursorIcon, Window, WindowId};

use crate::dom::NodeId;
use crate::fonts::FontStore;
use crate::layout::{find_link_node, hit_test, Rect};
use crate::page::{build_error_page, build_welcome_page, load_page_sized, Page, PageData};
use crate::paint::{draw_text_run, render_page, Pixmap};
use crate::style::ComputedStyle;
use crate::util::Color;

pub const CHROME_H: f32 = 56.0;

fn chrome_style(size: f32, color: Color) -> ComputedStyle {
    let mut s = ComputedStyle::default();
    s.font_size = size;
    s.font_family = "Segoe UI".to_string();
    s.color = color;
    s.line_height_factor = 1.2;
    s.line_height = size * 1.2;
    s
}

fn button_rects() -> (Rect, Rect, Rect, Rect) {
    let back = Rect::new(12.0, 11.0, 34.0, 34.0);
    let fwd = Rect::new(50.0, 11.0, 34.0, 34.0);
    let reload = Rect::new(88.0, 11.0, 34.0, 34.0);
    let addr = Rect::new(132.0, 10.0, 1.0, 36.0); // width computed at draw time
    (back, fwd, reload, addr)
}

enum UserEvent {
    Loaded(u64, Result<PageData, String>),
}

struct App {
    window: Option<Arc<Window>>,
    context: Option<softbuffer::Context<Arc<Window>>>,
    surface: Option<softbuffer::Surface<Arc<Window>, Arc<Window>>>,
    proxy: Option<EventLoopProxy<UserEvent>>,

    fonts: FontStore,
    page: Option<Page>,
    pixmap: Option<Pixmap>,
    width: f32,
    height: f32,
    scale: f32,
    modifiers: ModifiersState,

    url_input: String,
    editing: bool,
    editing_select_all: bool,
    editing_cursor: usize,

    history: Vec<String>,
    hist_index: usize,
    generation: u64,
    pending_history_add: bool,
    loading: bool,
    status: String,

    scroll: f32,
    mouse: (f32, f32),
    hovered_button: usize,
    hovered_node: Option<NodeId>,
    cursor_icon: CursorIcon,

    focused_input: Option<NodeId>,
    input_cursor: usize,

    initial_url: Option<String>,
    last_frame: Instant,
    loading_started: Instant,
    next_js_tick: Instant,
}

pub fn run(initial_url: Option<String>) {
    let event_loop = EventLoop::<UserEvent>::with_user_event().build().expect("event loop");
    event_loop.set_control_flow(ControlFlow::Wait);
    let proxy = event_loop.create_proxy();
    let mut app = App {
        window: None,
        context: None,
        surface: None,
        proxy: Some(proxy),
        fonts: FontStore::new(),
        page: None,
        pixmap: None,
        width: 1280.0,
        height: 800.0,
        scale: 1.0,
        modifiers: ModifiersState::default(),
        url_input: String::new(),
        editing: false,
        editing_select_all: false,
        editing_cursor: 0,
        history: Vec::new(),
        hist_index: 0,
        generation: 0,
        pending_history_add: true,
        loading: false,
        status: String::new(),
        scroll: 0.0,
        mouse: (-1.0, -1.0),
        hovered_button: 0,
        hovered_node: None,
        cursor_icon: CursorIcon::Default,
        focused_input: None,
        input_cursor: 0,
        initial_url,
        last_frame: Instant::now(),
        loading_started: Instant::now(),
        next_js_tick: Instant::now(),
    };
    event_loop.run_app(&mut app).expect("run app");
}

impl App {
    fn viewport(&self) -> (f32, f32) {
        ((self.width).max(100.0), (self.height - CHROME_H).max(100.0))
    }

    fn set_title(&self, loading: bool) {
        if let Some(win) = &self.window {
            let title = if let Some(page) = &self.page {
                let t = if page.title().is_empty() {
                    page.data.url.clone()
                } else {
                    page.title()
                };
                if loading {
                    format!("Loading… — {}", t)
                } else {
                    format!("{} — Aurora", t)
                }
            } else if loading {
                "Loading… — Aurora".to_string()
            } else {
                "Aurora".to_string()
            };
            win.set_title(&title);
        }
    }

    fn request_redraw(&self) {
        if let Some(w) = &self.window {
            w.request_redraw();
        }
    }

    fn load_start_page(&mut self) {
        let data = build_welcome_page();
        let vp = self.viewport();
        let page = Page::new(data, &mut self.fonts, vp);
        self.url_input = "aurora://start".to_string();
        self.history = vec!["aurora://start".to_string()];
        self.hist_index = 0;
        self.focused_input = None;
        self.scroll = 0.0;
        self.page = Some(page);
        self.set_title(false);
        if let Some(u) = self.initial_url.take() {
            self.navigate(&u, true);
        }
    }

    fn navigate(&mut self, input: &str, add_history: bool) {
        let input = input.trim().to_string();
        if input.is_empty() {
            return;
        }
        if input == "aurora://start" {
            let data = build_welcome_page();
            let vp = self.viewport();
            let page = Page::new(data, &mut self.fonts, vp);
            self.page = Some(page);
            self.url_input = "aurora://start".into();
            self.scroll = 0.0;
            self.focused_input = None;
            if add_history {
                self.history.truncate(self.hist_index + 1);
                self.history.push("aurora://start".into());
                self.hist_index = self.history.len() - 1;
            }
            self.set_title(false);
            self.request_redraw();
            return;
        }
        self.generation += 1;
        let gen = self.generation;
        self.loading = true;
        self.loading_started = Instant::now();
        self.status = format!("Loading {}…", input);
        self.pending_history_add = add_history;
        self.set_title(true);
        self.request_redraw();
        let proxy = self.proxy.as_ref().unwrap().clone();
        let current = self.page.as_ref().map(|p| p.data.url.clone());
        let input_cl = input.clone();
        let vp = self.viewport();
        thread::spawn(move || {
            let result = if input_cl == "aurora://start" {
                Ok(build_welcome_page())
            } else {
                load_page_sized(&input_cl, current.as_deref(), vp)
            };
            let _ = proxy.send_event(UserEvent::Loaded(gen, result));
        });
    }

    fn on_loaded(&mut self, gen: u64, result: Result<PageData, String>) {
        if gen != self.generation {
            return;
        }
        self.loading = false;
        let vp = self.viewport();
        let mut pending_nav: Option<String> = None;
        match result {
            Ok(data) => {
                let final_url = data.url.clone();
                let title = data.title.clone();
                let mut page = Page::new(data, &mut self.fonts, vp);
                pending_nav = page.take_pending_navigation();
                self.page = Some(page);
                self.url_input = final_url.clone();
                self.scroll = 0.0;
                self.focused_input = None;
                self.hovered_node = None;
                if self.pending_history_add {
                    self.history.truncate(self.hist_index + 1);
                    self.history.push(final_url);
                    self.hist_index = self.history.len() - 1;
                }
                self.status = if title.is_empty() {
                    String::new()
                } else {
                    format!("Loaded — {}", title)
                };
                self.next_js_tick = Instant::now() + Duration::from_millis(250);
            }
            Err(e) => {
                let error_page = build_error_page(&self.url_input, &e);
                let page = Page::new(error_page, &mut self.fonts, vp);
                self.page = Some(page);
                self.focused_input = None;
                self.status = e;
            }
        }
        self.set_title(false);
        self.request_redraw();
        let nav = pending_nav.or_else(|| {
            self.page.as_ref().and_then(|p| p.meta_refresh_url())
        });
        if let Some(nav) = nav {
            let current = self.page.as_ref().map(|p| p.data.url.clone()).unwrap_or_default();
            if nav != current {
                self.navigate(&nav, true);
            }
        }
    }

    fn reload(&mut self) {
        let url = self
            .page
            .as_ref()
            .map(|p| p.data.url.clone())
            .filter(|u| u != "aurora://start")
            .unwrap_or_else(|| self.url_input.clone());
        if !url.is_empty() {
            self.navigate(&url, false);
        }
    }

    fn go_back(&mut self) {
        if self.hist_index > 0 {
            self.hist_index -= 1;
            let url = self.history[self.hist_index].clone();
            self.navigate(&url, false);
        }
    }

    fn go_forward(&mut self) {
        if self.hist_index + 1 < self.history.len() {
            self.hist_index += 1;
            let url = self.history[self.hist_index].clone();
            self.navigate(&url, false);
        }
    }

    fn max_scroll(&self) -> f32 {
        if let Some(page) = &self.page {
            (page.layout.page_height - self.viewport().1).max(0.0)
        } else {
            0.0
        }
    }

    fn scroll_by(&mut self, delta: f32) {
        let max = self.max_scroll();
        self.scroll = (self.scroll + delta).clamp(0.0, max);
        self.request_redraw();
    }

    fn address_rect(&self) -> Rect {
        Rect::new(132.0, 10.0, (self.width - 144.0).max(60.0), 36.0)
    }

    fn render(&mut self) {
        let logical_w = (self.width.max(320.0)) as usize;
        let logical_h = (self.height.max(200.0)) as usize;
        let mut pix = match self.pixmap.take() {
            Some(p) => p,
            None => Pixmap::new(logical_w, logical_h, Color::WHITE),
        };
        if pix.width != logical_w || pix.height != logical_h {
            pix.resize(logical_w, logical_h, Color::WHITE);
        }
        pix.clear(Color::WHITE);

        // Browser chrome background.
        let toolbar = Rect::new(0.0, 0.0, logical_w as f32, CHROME_H);
        pix.fill_rect(toolbar, Color::rgb(243, 244, 246));
        pix.fill_rect(
            Rect::new(0.0, CHROME_H - 1.0, logical_w as f32, 1.0),
            Color::rgb(209, 213, 219),
        );

        // Page viewport.
        let vp = self.viewport();
        if let Some(page) = &mut self.page {
            if (page.viewport.0 - vp.0).abs() > 0.5 || (page.viewport.1 - vp.1).abs() > 0.5 {
                page.relayout(&mut self.fonts, vp);
            }
            let max = (page.layout.page_height - vp.1).max(0.0);
            if self.scroll > max {
                self.scroll = max;
            }
            {
                let doc = page.dom.borrow();
                render_page(
                    &mut pix,
                    &page.layout,
                    &doc,
                    &mut self.fonts,
                    &page.data.images,
                    self.scroll,
                    CHROME_H,
                );
            }
        } else {
            pix.fill_rect(
                Rect::new(0.0, CHROME_H, logical_w as f32, logical_h as f32 - CHROME_H),
                Color::rgb(238, 241, 247),
            );
        }

        self.draw_chrome(&mut pix);
        self.draw_scrollbar(&mut pix);

        // Present to the window at physical resolution.
        if let (Some(surface), Some(window)) = (&mut self.surface, &self.window) {
            let size = window.inner_size();
            let (pw, ph) = (size.width.max(1), size.height.max(1));
            let _ = surface.resize(
                NonZeroU32::new(pw).unwrap(),
                NonZeroU32::new(ph).unwrap(),
            );
            if let Ok(mut buffer) = surface.buffer_mut() {
                let dst_w = pw as usize;
                let dst_h = ph as usize;
                let src_w = pix.width;
                let src_h = pix.height;
                if dst_w == src_w && dst_h == src_h {
                    buffer.copy_from_slice(&pix.data);
                } else {
                    let sx_scale = src_w as f32 / dst_w as f32;
                    let sy_scale = src_h as f32 / dst_h as f32;
                    for y in 0..dst_h {
                        let sy = ((y as f32 + 0.5) * sy_scale - 0.5).clamp(0.0, src_h as f32 - 1.0);
                        let y0 = sy.floor() as usize;
                        let y1 = (y0 + 1).min(src_h - 1);
                        let fy = sy - y0 as f32;
                        for x in 0..dst_w {
                            let sx = ((x as f32 + 0.5) * sx_scale - 0.5).clamp(0.0, src_w as f32 - 1.0);
                            let x0 = sx.floor() as usize;
                            let x1 = (x0 + 1).min(src_w - 1);
                            let fx = sx - x0 as f32;
                            let p00 = pix.data[y0 * src_w + x0];
                            let p10 = pix.data[y0 * src_w + x1];
                            let p01 = pix.data[y1 * src_w + x0];
                            let p11 = pix.data[y1 * src_w + x1];
                            let mix = |a: u32, b: u32, c: u32, d: u32| -> u32 {
                                let r = ((((a >> 16) & 0xFF) as f32) * (1.0 - fx) + (((b >> 16) & 0xFF) as f32) * fx) * (1.0 - fy)
                                    + ((((c >> 16) & 0xFF) as f32) * (1.0 - fx) + (((d >> 16) & 0xFF) as f32) * fx) * fy;
                                let g = ((((a >> 8) & 0xFF) as f32) * (1.0 - fx) + (((b >> 8) & 0xFF) as f32) * fx) * (1.0 - fy)
                                    + ((((c >> 8) & 0xFF) as f32) * (1.0 - fx) + (((d >> 8) & 0xFF) as f32) * fx) * fy;
                                let bl = (((a & 0xFF) as f32) * (1.0 - fx) + ((b & 0xFF) as f32) * fx) * (1.0 - fy)
                                    + (((c & 0xFF) as f32) * (1.0 - fx) + ((d & 0xFF) as f32) * fx) * fy;
                                ((r.round() as u32) << 16) | ((g.round() as u32) << 8) | (bl.round() as u32)
                            };
                            buffer[y * dst_w + x] = mix(p00, p10, p01, p11);
                        }
                    }
                }
                let _ = buffer.present();
            }
        }
        self.pixmap = Some(pix);
        self.last_frame = Instant::now();
    }

    fn draw_chrome(&mut self, pix: &mut Pixmap) {
        let w = pix.width as f32;
        let (back, fwd, reload, _addr) = button_rects();
        let addr = self.address_rect();
        let hover = self.mouse;
        let mut draw_button = |pix: &mut Pixmap, fonts: &mut FontStore, r: Rect, label: &str, enabled: bool, hot: bool| {
            if hot {
                pix.fill_rounded_rect(r, 8.0, Color::rgb(226, 228, 232), Rect::new(0.0, 0.0, w, CHROME_H));
            }
            let color = if enabled { Color::rgb(55, 61, 70) } else { Color::rgb(176, 181, 189) };
            let style = chrome_style(19.0, color);
            let tw = fonts.measure_style(label, &style);
            let (asc, desc) = fonts.line_metrics(&style);
            let baseline = r.y + r.h / 2.0 + (asc - desc) / 2.0 - 1.0;
            draw_text_run(
                pix,
                fonts,
                label,
                &style,
                r.x + (r.w - tw) / 2.0,
                baseline,
                color,
                Rect::new(0.0, 0.0, w, CHROME_H),
            );
        };
        let back_hot = back.contains(hover.0, hover.1);
        let fwd_hot = fwd.contains(hover.0, hover.1);
        let reload_hot = reload.contains(hover.0, hover.1);
        let can_back = self.hist_index > 0;
        let can_fwd = self.hist_index + 1 < self.history.len();
        {
            let fonts = &mut self.fonts;
            draw_button(pix, fonts, back, "←", can_back, back_hot);
            draw_button(pix, fonts, fwd, "→", can_fwd, fwd_hot);
            draw_button(pix, fonts, reload, "↻", true, reload_hot);
        }

        // Address bar.
        let address_hot = addr.contains(hover.0, hover.1);
        pix.fill_rounded_rect(addr, 9.0, Color::WHITE, Rect::new(0.0, 0.0, w, CHROME_H));
        let border_color = if self.editing {
            Color::rgb(64, 110, 220)
        } else if address_hot {
            Color::rgb(150, 160, 175)
        } else {
            Color::rgb(203, 209, 218)
        };
        // Rounded border approximation.
        pix.stroke_rect(addr, 1.0, border_color, Rect::new(0.0, 0.0, w, CHROME_H));

        let base_style = chrome_style(14.5, Color::rgb(28, 34, 44));
        let placeholder_style = chrome_style(14.5, Color::rgb(135, 143, 156));
        let full_clip = Rect::new(0.0, 0.0, w, CHROME_H);
        let text_clip = Rect::new(addr.x + 30.0, addr.y + 2.0, (addr.w - 48.0).max(10.0), addr.h - 4.0);
        // Draw a small globe-ish search icon.
        let icon_style = chrome_style(14.0, Color::rgb(120, 128, 140));
        {
            let fonts = &mut self.fonts;
            let (asc, desc) = fonts.line_metrics(&base_style);
            let baseline = addr.y + addr.h / 2.0 + (asc - desc) / 2.0 - 1.0;
            draw_text_run(pix, fonts, "⌕", &icon_style, addr.x + 10.0, baseline, icon_style.color, full_clip);
            let text = if self.editing {
                self.url_input.clone()
            } else if self.url_input == "aurora://start" {
                String::new()
            } else {
                self.url_input.clone()
            };
            let (display, style) = if text.is_empty() {
                ("Search or enter address".to_string(), &placeholder_style)
            } else {
                (text, &base_style)
            };
            let mut offset = 0.0;
            if self.editing {
                let chars: Vec<char> = display.chars().collect();
                let upto: String = chars[..self.editing_cursor.min(chars.len())].iter().collect();
                let caret_x = fonts.measure_style(&upto, &base_style);
                if caret_x > text_clip.w {
                    offset = caret_x - text_clip.w;
                }
            }
            draw_text_run(pix, fonts, &display, style, text_clip.x - offset, baseline, style.color, text_clip);
            if self.editing {
                let chars: Vec<char> = display.chars().collect();
                let upto: String = chars[..self.editing_cursor.min(chars.len())].iter().collect();
                let caret_x = text_clip.x - offset + fonts.measure_style(&upto, &base_style);
                pix.fill_rect(
                    Rect::new(caret_x.round(), addr.y + 8.0, 1.0, addr.h - 16.0),
                    Color::rgb(30, 40, 60),
                );
            }
            if self.loading {
                let load_style = chrome_style(12.0, Color::rgb(100, 108, 120));
                draw_text_run(
                    pix,
                    fonts,
                    "Loading…",
                    &load_style,
                    addr.right() - 62.0,
                    baseline,
                    load_style.color,
                    full_clip,
                );
            }
        }
        // Loading progress bar along the bottom edge.
        if self.loading {
            let elapsed = self.loading_started.elapsed().as_secs_f32();
            let progress = (elapsed / 2.5).min(0.94);
            pix.fill_rect(
                Rect::new(0.0, CHROME_H - 2.0, w * progress, 2.0),
                Color::rgb(70, 110, 220),
            );
        }
    }

    fn draw_scrollbar(&mut self, pix: &mut Pixmap) {
        let page = match &self.page {
            Some(p) => p,
            None => return,
        };
        let vp_h = self.viewport().1;
        let page_h = page.layout.page_height;
        if page_h <= vp_h + 1.0 {
            return;
        }
        let h = pix.height as f32;
        let w = pix.width as f32;
        let track_y = CHROME_H + 4.0;
        let track_h = h - CHROME_H - 8.0;
        let thumb_h = (vp_h / page_h * track_h).max(30.0);
        let max_scroll = page_h - vp_h;
        let frac = if max_scroll > 0.0 { (self.scroll / max_scroll).clamp(0.0, 1.0) } else { 0.0 };
        let thumb_y = track_y + frac * (track_h - thumb_h);
        pix.fill_rounded_rect(
            Rect::new(w - 9.0, thumb_y, 6.0, thumb_h),
            3.0,
            Color::rgba(120, 126, 136, 150),
            Rect::new(0.0, CHROME_H, w, h - CHROME_H),
        );
    }

    fn page_coords(&self) -> (f32, f32) {
        (self.mouse.0, self.mouse.1 - CHROME_H + self.scroll)
    }

    fn node_at(&self, x: f32, y: f32) -> Option<NodeId> {
        let page = self.page.as_ref()?;
        hit_test(&page.layout.root, x, y)
    }

    fn ancestor_with_tag(&self, node: NodeId, tags: &[&str]) -> Option<NodeId> {
        let page = self.page.as_ref()?;
        let mut cur = Some(node);
        let doc = page.dom.borrow();
        while let Some(n) = cur {
            if let Some(t) = doc.tag(n) {
                if tags.contains(&t) {
                    return Some(n);
                }
            }
            cur = doc.nodes[n].parent;
        }
        None
    }

    fn update_hover(&mut self) {
        let (px, py) = self.page_coords();
        let mut new_hover = None;
        let mut new_icon = CursorIcon::Default;
        let mut new_button = 0;
        if self.mouse.1 < CHROME_H {
            let (back, fwd, reload, _) = button_rects();
            let addr = self.address_rect();
            if back.contains(self.mouse.0, self.mouse.1) {
                new_button = 1;
                new_icon = CursorIcon::Pointer;
            } else if fwd.contains(self.mouse.0, self.mouse.1) {
                new_button = 2;
                new_icon = CursorIcon::Pointer;
            } else if reload.contains(self.mouse.0, self.mouse.1) {
                new_button = 3;
                new_icon = CursorIcon::Pointer;
            } else if addr.contains(self.mouse.0, self.mouse.1) {
                new_icon = CursorIcon::Text;
            }
        } else if let Some(page) = &self.page {
            if py >= 0.0 && py <= page.layout.page_height + 200.0 {
                if let Some(node) = hit_test(&page.layout.root, px, py) {
                    new_hover = Some(node);
                    if find_link_node(&page.dom.borrow(), node).is_some() {
                        new_icon = CursorIcon::Pointer;
                    } else if self
                        .ancestor_with_tag(node, &["input", "textarea", "select"])
                        .is_some()
                    {
                        new_icon = CursorIcon::Text;
                    }
                }
            }
        }
        self.hovered_button = new_button;
        if let Some(win) = &self.window {
            if new_icon != self.cursor_icon {
                self.cursor_icon = new_icon;
                win.set_cursor(new_icon);
            }
        }
        if new_hover != self.hovered_node {
            self.hovered_node = new_hover;
            let vp = self.viewport();
            let (page_opt, fonts) = (&mut self.page, &mut self.fonts);
            if let Some(page) = page_opt {
                page.set_hover(fonts, new_hover);
            }
            let _ = vp;
        }
    }

    fn click_toolbar(&mut self, x: f32, y: f32) {
        let (back, fwd, reload, _) = button_rects();
        let addr = self.address_rect();
        if back.contains(x, y) {
            self.go_back();
        } else if fwd.contains(x, y) {
            self.go_forward();
        } else if reload.contains(x, y) {
            self.reload();
        } else if addr.contains(x, y) {
            self.editing = true;
            self.editing_select_all = true;
            self.editing_cursor = self.url_input.chars().count();
            self.focused_input = None;
            self.request_redraw();
        }
    }

    fn click_page(&mut self, x: f32, y: f32) {
        let scroll = self.scroll;
        let (px, py) = (x, y - CHROME_H + scroll);
        let node = self.node_at(px, py);
        let node = match node {
            Some(n) => n,
            None => {
                self.blur_input();
                return;
            }
        };
        // Let page scripts see the click first.
        let prevented = {
            let vp = self.viewport();
            let (page_opt, fonts) = (&mut self.page, &mut self.fonts);
            if let Some(page) = page_opt {
                let prevented = page.dispatch_js(Some(node), "click");
                if page.take_js_changed() {
                    page.relayout(fonts, vp);
                }
                prevented
            } else {
                false
            }
        };
        if prevented {
            self.set_title(false);
            self.request_redraw();
            return;
        }
        let pending = self
            .page
            .as_mut()
            .and_then(|p| p.take_pending_navigation());
        if let Some(url) = pending {
            self.navigate(&url, true);
            return;
        }
        // Submit buttons / inputs.
        let submit = {
            let page = match &self.page {
                Some(p) => p,
                None => return,
            };
            let doc = page.dom.borrow();
            let mut form = None;
            if doc.tag(node) == Some("button") {
                let ty = doc.attr(node, "type").unwrap_or("submit").to_ascii_lowercase();
                if ty == "submit" {
                    form = page.ancestor_tag(node, "form");
                }
            } else if doc.tag(node) == Some("input") {
                let ty = doc.attr(node, "type").unwrap_or("text").to_ascii_lowercase();
                if ty == "submit" || ty == "image" {
                    form = page.ancestor_tag(node, "form");
                }
            }
            form.and_then(|f| page.form_submit_url(f))
        };
        if let Some(url) = submit {
            self.navigate(&url, true);
            return;
        }
        // Links.
        let link_target = if let Some(page) = &self.page {
            let doc = page.dom.borrow();
            find_link_node(&doc, node).and_then(|link| page.link_href(link))
        } else {
            None
        };
        if let Some(href) = link_target {
            if href.starts_with('#') {
                self.scroll_to_anchor(&href[1..]);
                return;
            }
            self.navigate(&href, true);
            return;
        }
        // Form controls.
        if let Some(input) = self.ancestor_with_tag(node, &["input"]) {
            let itype = self
                .page
                .as_ref()
                .and_then(|p| p.dom.borrow().attr(input, "type").map(|t| t.to_ascii_lowercase()))
                .unwrap_or_else(|| "text".to_string());
            if itype == "checkbox" || itype == "radio" {
                self.toggle_checkbox(input);
                return;
            }
            self.focus_input(input);
            return;
        }
        if let Some(area) = self.ancestor_with_tag(node, &["textarea"]) {
            self.focus_input(area);
            return;
        }
        if let Some(sel) = self.ancestor_with_tag(node, &["select"]) {
            self.cycle_select(sel);
            return;
        }
        if self.ancestor_with_tag(node, &["button"]).is_some() {
            return;
        }
        self.blur_input();
    }

    fn scroll_to_anchor(&mut self, id: &str) {
        let target = self
            .page
            .as_ref()
            .and_then(|p| p.dom.borrow().find_by_id(id));
        if let Some(node) = target {
            if let Some(page) = &self.page {
                if let Some(rect) = find_box_rect(&page.layout.root, node) {
                    let max = self.max_scroll();
                    self.scroll = (rect.y - 8.0).clamp(0.0, max);
                    self.request_redraw();
                    return;
                }
            }
        }
    }

    fn focus_input(&mut self, node: NodeId) {
        self.focused_input = Some(node);
        self.editing = false;
        let value = self
            .page
            .as_ref()
            .and_then(|p| p.dom.borrow().attr(node, "value").map(|s| s.to_string()))
            .unwrap_or_default();
        self.input_cursor = value.chars().count();
        let vp = self.viewport();
        let (page_opt, fonts) = (&mut self.page, &mut self.fonts);
        if let Some(page) = page_opt {
            page.dispatch_js(Some(node), "focus");
            page.set_focused(fonts, Some(node));
            let _ = vp;
        }
        self.request_redraw();
    }

    fn blur_input(&mut self) {
        if self.focused_input.is_some() {
            let node = self.focused_input.take();
            let vp = self.viewport();
            if let Some(node) = node {
                if let Some(page) = &mut self.page {
                    page.dispatch_js(Some(node), "change");
                    page.dispatch_js(Some(node), "blur");
                }
            }
            let (page_opt, fonts) = (&mut self.page, &mut self.fonts);
            if let Some(page) = page_opt {
                page.set_focused(fonts, None);
                let _ = vp;
            }
            self.request_redraw();
        }
    }

    fn toggle_checkbox(&mut self, node: NodeId) {
        let checked = self
            .page
            .as_ref()
            .and_then(|p| p.dom.borrow().attr(node, "checked").map(|_| true))
            .unwrap_or(false);
        if let Some(page) = &mut self.page {
            {
                let mut doc = page.dom.borrow_mut();
                if checked {
                    doc.remove_attr(node, "checked");
                } else {
                    doc.set_attr(node, "checked", "");
                }
            }
            let vp = self.viewport();
            let (page_opt, fonts) = (&mut self.page, &mut self.fonts);
            if let Some(page) = page_opt {
                page.relayout(fonts, vp);
            }
        }
        self.request_redraw();
    }

    fn cycle_select(&mut self, node: NodeId) {
        let options: Vec<(NodeId, String)> = {
            let page = match &self.page {
                Some(p) => p,
                None => return,
            };
            let doc = page.dom.borrow();
            doc.nodes[node]
                .children
                .iter()
                .filter(|&&c| doc.tag(c) == Some("option"))
                .map(|&c| {
                    let text = doc.text_content(c).trim().to_string();
                    (c, text)
                })
                .collect()
        };
        if options.is_empty() {
            return;
        }
        if let Some(page) = &mut self.page {
            {
                let mut doc = page.dom.borrow_mut();
                let mut current = 0;
                for (i, (c, _)) in options.iter().enumerate() {
                    if doc.attr(*c, "selected").is_some() {
                        current = i;
                    }
                }
                let next = (current + 1) % options.len();
                for (i, (c, _)) in options.iter().enumerate() {
                    if i == next {
                        doc.set_attr(*c, "selected", "");
                    } else {
                        doc.remove_attr(*c, "selected");
                    }
                }
            }
            let vp = self.viewport();
            let (page_opt, fonts) = (&mut self.page, &mut self.fonts);
            if let Some(page) = page_opt {
                page.relayout(fonts, vp);
            }
        }
        self.request_redraw();
    }

    fn type_into_input(&mut self, text: &str) {
        let node = match self.focused_input {
            Some(n) => n,
            None => return,
        };
        let is_textarea = self
            .page
            .as_ref()
            .and_then(|p| p.dom.borrow().tag(node).map(|t| t == "textarea"))
            .unwrap_or(false);
        if let Some(page) = &mut self.page {
            {
                let mut doc = page.dom.borrow_mut();
                if is_textarea {
                    let value = doc.text_content(node);
                    self.input_cursor = self.input_cursor.min(value.chars().count());
                    let mut chars: Vec<char> = value.chars().collect();
                    let mut insert_at = self.input_cursor.min(chars.len());
                    for c in text.chars() {
                        if c == '\r' {
                            continue;
                        }
                        chars.insert(insert_at, c);
                        insert_at += 1;
                    }
                    let new_value: String = chars.into_iter().collect();
                    doc.set_text_content(node, &new_value);
                    self.input_cursor = insert_at;
                } else {
                    let value = doc.attr(node, "value").unwrap_or("").to_string();
                    let mut chars: Vec<char> = value.chars().collect();
                    self.input_cursor = self.input_cursor.min(chars.len());
                    let mut insert_at = self.input_cursor;
                    for c in text.chars() {
                        if c == '\r' || c == '\n' {
                            continue;
                        }
                        chars.insert(insert_at, c);
                        insert_at += 1;
                    }
                    let new_value: String = chars.into_iter().collect();
                    doc.set_attr(node, "value", &new_value);
                    self.input_cursor = insert_at;
                }
            }
            let vp = self.viewport();
            let (page_opt, fonts) = (&mut self.page, &mut self.fonts);
            if let Some(page) = page_opt {
                page.dispatch_js(Some(node), "input");
                page.relayout(fonts, vp);
            }
        }
        self.request_redraw();
    }

    fn backspace_input(&mut self) {
        let node = match self.focused_input {
            Some(n) => n,
            None => return,
        };
        let is_textarea = self
            .page
            .as_ref()
            .and_then(|p| p.dom.borrow().tag(node).map(|t| t == "textarea"))
            .unwrap_or(false);
        if let Some(page) = &mut self.page {
            {
                let mut doc = page.dom.borrow_mut();
                if is_textarea {
                    let value = doc.text_content(node);
                    let mut chars: Vec<char> = value.chars().collect();
                    if !chars.is_empty() {
                        chars.pop();
                    }
                    let new_value: String = chars.into_iter().collect();
                    doc.set_text_content(node, &new_value);
                    self.input_cursor = new_value.chars().count();
                } else {
                    let value = doc.attr(node, "value").unwrap_or("").to_string();
                    let mut chars: Vec<char> = value.chars().collect();
                    if !chars.is_empty() {
                        chars.pop();
                    }
                    let new_value: String = chars.into_iter().collect();
                    doc.set_attr(node, "value", &new_value);
                    self.input_cursor = new_value.chars().count();
                }
            }
            let vp = self.viewport();
            let (page_opt, fonts) = (&mut self.page, &mut self.fonts);
            if let Some(page) = page_opt {
                page.dispatch_js(Some(node), "input");
                page.relayout(fonts, vp);
            }
        }
        self.request_redraw();
    }

    fn on_key(&mut self, event: &winit::event::KeyEvent) {
        let mods = self.modifiers;
        let ctrl = mods.control_key();
        let alt = mods.alt_key();
        let shift = mods.shift_key();
        let key = &event.logical_key;
        if ctrl {
            match key {
                Key::Character(s) => match s.to_ascii_lowercase().as_str() {
                    "l" => {
                        self.editing = true;
                        self.editing_select_all = true;
                        self.editing_cursor = self.url_input.chars().count();
                        self.focused_input = None;
                        self.request_redraw();
                        return;
                    }
                    "r" => {
                        self.reload();
                        return;
                    }
                    "a" if self.editing => {
                        self.editing_select_all = true;
                        self.request_redraw();
                        return;
                    }
                    _ => {}
                },
                _ => {}
            }
        }
        if alt && matches!(key, Key::Named(NamedKey::ArrowLeft)) {
            self.go_back();
            return;
        }
        if alt && matches!(key, Key::Named(NamedKey::ArrowRight)) {
            self.go_forward();
            return;
        }
        match key {
            Key::Named(NamedKey::F5) => {
                self.reload();
                return;
            }
            Key::Named(NamedKey::Escape) => {
                if self.editing {
                    self.editing = false;
                } else {
                    self.blur_input();
                }
                self.request_redraw();
                return;
            }
            _ => {}
        }
        if self.editing {
            match key {
                Key::Named(NamedKey::Enter) => {
                    let input = self.url_input.clone();
                    self.editing = false;
                    self.editing_select_all = false;
                    self.navigate(&input, true);
                }
                Key::Named(NamedKey::Backspace) => {
                    let mut chars: Vec<char> = self.url_input.chars().collect();
                    if self.editing_select_all {
                        chars.clear();
                        self.editing_select_all = false;
                    } else if self.editing_cursor > 0 && self.editing_cursor <= chars.len() {
                        chars.remove(self.editing_cursor - 1);
                        self.editing_cursor -= 1;
                    }
                    self.url_input = chars.into_iter().collect();
                    self.request_redraw();
                }
                Key::Named(NamedKey::Delete) => {
                    let mut chars: Vec<char> = self.url_input.chars().collect();
                    if self.editing_cursor < chars.len() {
                        chars.remove(self.editing_cursor);
                    }
                    self.url_input = chars.into_iter().collect();
                    self.request_redraw();
                }
                Key::Named(NamedKey::ArrowLeft) => {
                    self.editing_cursor = self.editing_cursor.saturating_sub(1);
                    self.request_redraw();
                }
                Key::Named(NamedKey::ArrowRight) => {
                    self.editing_cursor =
                        (self.editing_cursor + 1).min(self.url_input.chars().count());
                    self.request_redraw();
                }
                Key::Named(NamedKey::Home) => {
                    self.editing_cursor = 0;
                    self.request_redraw();
                }
                Key::Named(NamedKey::End) => {
                    self.editing_cursor = self.url_input.chars().count();
                    self.request_redraw();
                }
                _ => {
                    if let Some(text) = &event.text {
                        let mut inserted = false;
                        let chars: Vec<char> = text.chars().collect();
                        if ctrl {
                            return;
                        }
                        if self.editing_select_all {
                            self.url_input.clear();
                            self.editing_cursor = 0;
                            self.editing_select_all = false;
                        }
                        let mut current: Vec<char> = self.url_input.chars().collect();
                        let mut cursor = self.editing_cursor.min(current.len());
                        for c in chars {
                            if c.is_control() {
                                continue;
                            }
                            current.insert(cursor, c);
                            cursor += 1;
                            inserted = true;
                        }
                        if inserted {
                            self.url_input = current.into_iter().collect();
                            self.editing_cursor = cursor;
                        }
                        if inserted {
                            self.request_redraw();
                        }
                    }
                }
            }
            return;
        }
        if self.focused_input.is_some() {
            match key {
                Key::Named(NamedKey::Enter) => {
                    let submit = self.focused_input.and_then(|node| {
                        let page = self.page.as_ref()?;
                        let form = page.ancestor_tag(node, "form")?;
                        page.form_submit_url(form)
                    });
                    if let Some(url) = submit {
                        self.navigate(&url, true);
                    } else {
                        self.blur_input();
                    }
                }
                Key::Named(NamedKey::Backspace) => {
                    self.backspace_input();
                }
                _ => {
                    if let Some(text) = &event.text {
                        if !ctrl && !alt {
                            let filtered: String = text.chars().filter(|c| !c.is_control() || *c == ' ').collect();
                            if !filtered.is_empty() {
                                self.type_into_input(&filtered);
                            }
                        }
                    }
                }
            }
            return;
        }
        // Page keyboard scrolling.
        match key {
            Key::Named(NamedKey::ArrowDown) => self.scroll_by(50.0),
            Key::Named(NamedKey::ArrowUp) => self.scroll_by(-50.0),
            Key::Named(NamedKey::PageDown) | Key::Named(NamedKey::Space) => {
                self.scroll_by(self.viewport().1 * 0.9)
            }
            Key::Named(NamedKey::PageUp) => self.scroll_by(-self.viewport().1 * 0.9),
            Key::Named(NamedKey::Home) => {
                self.scroll = 0.0;
                self.request_redraw();
            }
            Key::Named(NamedKey::End) => {
                self.scroll = self.max_scroll();
                self.request_redraw();
            }
            Key::Named(NamedKey::ArrowRight) => self.scroll_by(40.0),
            Key::Named(NamedKey::ArrowLeft) => self.scroll_by(-40.0),
            _ => {}
        }
        let _ = shift;
    }
}

fn chars_len(s: &str) -> usize {
    s.chars().count()
}

fn find_box_rect(b: &crate::layout::LayoutBox, node: NodeId) -> Option<Rect> {
    if b.node == node {
        return Some(b.rect);
    }
    for c in &b.children {
        if let Some(r) = find_box_rect(c, node) {
            return Some(r);
        }
    }
    None
}

impl ApplicationHandler<UserEvent> for App {
    fn resumed(&mut self, event_loop: &ActiveEventLoop) {
        if self.window.is_some() {
            return;
        }
        let attrs = Window::default_attributes()
            .with_title("Aurora")
            .with_inner_size(LogicalSize::new(1280.0, 820.0))
            .with_min_inner_size(LogicalSize::new(480.0, 320.0));
        let window = match event_loop.create_window(attrs) {
            Ok(w) => Arc::new(w),
            Err(e) => {
                eprintln!("Failed to create window: {}", e);
                event_loop.exit();
                return;
            }
        };
        let context = match softbuffer::Context::new(window.clone()) {
            Ok(c) => c,
            Err(e) => {
                eprintln!("Failed to create drawing context: {}", e);
                event_loop.exit();
                return;
            }
        };
        let surface = match softbuffer::Surface::new(&context, window.clone()) {
            Ok(s) => s,
            Err(e) => {
                eprintln!("Failed to create surface: {}", e);
                event_loop.exit();
                return;
            }
        };
        let scale = window.scale_factor() as f32;
        let size = window.inner_size();
        self.width = size.width as f32 / scale;
        self.height = size.height as f32 / scale;
        self.scale = scale;
        self.context = Some(context);
        self.surface = Some(surface);
        self.window = Some(window);
        self.load_start_page();
        self.request_redraw();
    }

    fn window_event(
        &mut self,
        event_loop: &ActiveEventLoop,
        _window_id: WindowId,
        event: WindowEvent,
    ) {
        match event {
            WindowEvent::CloseRequested => event_loop.exit(),
            WindowEvent::Resized(size) => {
                self.width = size.width as f32 / self.scale;
                self.height = size.height as f32 / self.scale;
                self.request_redraw();
            }
            WindowEvent::ScaleFactorChanged { scale_factor, .. } => {
                self.scale = scale_factor as f32;
                if let Some(win) = &self.window {
                    let size = win.inner_size();
                    self.width = size.width as f32 / self.scale;
                    self.height = size.height as f32 / self.scale;
                }
                self.request_redraw();
            }
            WindowEvent::RedrawRequested => {
                self.render();
            }
            WindowEvent::KeyboardInput { event, .. } => {
                if event.state == ElementState::Pressed {
                    self.on_key(&event);
                }
            }
            WindowEvent::ModifiersChanged(mods) => {
                self.modifiers = mods.state();
            }
            WindowEvent::CursorMoved { position, .. } => {
                self.mouse = (
                    position.x as f32 / self.scale,
                    position.y as f32 / self.scale,
                );
                self.update_hover();
                self.request_redraw();
            }
            WindowEvent::CursorLeft { .. } => {
                self.mouse = (-1.0, -1.0);
                self.update_hover();
                self.request_redraw();
            }
            WindowEvent::MouseInput { state, button, .. } => {
                if state == ElementState::Pressed && button == MouseButton::Left {
                    let (x, y) = self.mouse;
                    if y < CHROME_H {
                        self.click_toolbar(x, y);
                    } else {
                        self.click_page(x, y);
                    }
                    self.request_redraw();
                }
            }
            WindowEvent::MouseWheel { delta, .. } => {
                let dy = match delta {
                    MouseScrollDelta::LineDelta(_, y) => y * 48.0,
                    MouseScrollDelta::PixelDelta(p) => p.y as f32 / self.scale,
                };
                self.scroll_by(-dy);
            }
            _ => {}
        }
    }

    fn user_event(&mut self, _event_loop: &ActiveEventLoop, event: UserEvent) {
        match event {
            UserEvent::Loaded(gen, result) => self.on_loaded(gen, result),
        }
    }

    fn about_to_wait(&mut self, event_loop: &ActiveEventLoop) {
        if self.loading {
            if let Some(win) = &self.window {
                win.request_redraw();
            }
            event_loop.set_control_flow(ControlFlow::WaitUntil(
                Instant::now() + Duration::from_millis(80),
            ));
            return;
        }
        let has_timers = self.page.as_ref().map(|p| p.has_js_timers()).unwrap_or(false);
        if has_timers && Instant::now() >= self.next_js_tick {
            self.next_js_tick = Instant::now() + Duration::from_millis(250);
            let changed = {
                let (page_opt, fonts) = (&mut self.page, &mut self.fonts);
                if let Some(page) = page_opt {
                    page.js_tick(fonts)
                } else {
                    false
                }
            };
            if changed {
                self.set_title(false);
                self.request_redraw();
            }
            if self.page.as_ref().map(|p| p.has_js_timers()).unwrap_or(false) {
                event_loop.set_control_flow(ControlFlow::WaitUntil(self.next_js_tick));
                return;
            }
        }
        event_loop.set_control_flow(ControlFlow::Wait);
    }
}
