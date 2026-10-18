use eframe::egui::{self, Color32, CornerRadius, Margin, RichText, Sense, Stroke, Vec2};

use crate::fractal::{self, Kind, JULIA_PRESETS, MANDELBROT_PLACES, PALETTES};
use crate::gpu;

// UI palette (inspired by Zed's One Dark).
const BG_EDITOR: Color32 = Color32::from_rgb(0x28, 0x2c, 0x33);
const BG_PANEL: Color32 = Color32::from_rgb(0x2f, 0x34, 0x3e);
const BG_ELEVATED: Color32 = Color32::from_rgb(0x3b, 0x41, 0x4d);
const BORDER: Color32 = Color32::from_rgb(0x36, 0x3c, 0x46);
const TEXT: Color32 = Color32::from_rgb(0xdc, 0xe0, 0xe5);
const TEXT_MUTED: Color32 = Color32::from_rgb(0xa9, 0xaf, 0xbc);
const ACCENT: Color32 = Color32::from_rgb(0x74, 0xad, 0xe8);

/// Smallest pixel size in the complex plane before f32 precision degrades the image.
const MIN_PIXEL_STEP: f64 = 2.0e-7;
/// Same in perturbation mode, limited by the f64 precision of the view center.
const MIN_PIXEL_STEP_DEEP: f64 = 2.0e-16;
/// Below this per-pixel step, Mandelbrot/Julia switch to perturbation mode.
const DEEP_THRESHOLD: f64 = 2.0e-6;
const ORBIT_BAILOUT_SQ: f64 = 256.0 * 256.0;

#[derive(Clone, Copy)]
struct View {
    center: [f64; 2],
    /// Height of the visible area, in complex-plane units.
    height: f64,
}

pub struct App {
    kind: Kind,
    view: View,
    target: View,

    julia_c: [f64; 2],
    rotate_c: bool,
    rotate_phase: f64,
    power: f32,

    auto_iter: bool,
    max_iter: u32,
    ssaa: u32,
    light_while_moving: bool,

    palette: usize,
    color_scale: f32,
    color_offset: f32,
    animate_colors: bool,
    anim_phase: f32,

    show_panel: bool,
    cursor_c: Option<[f64; 2]>,
    at_precision_limit: bool,
    deep_mode: bool,
    adapter_name: String,
}

impl App {
    pub fn new(cc: &eframe::CreationContext<'_>) -> Self {
        setup_style(&cc.egui_ctx);
        let mut adapter_name = String::from("GPU");
        if let Some(rs) = cc.wgpu_render_state.as_ref() {
            gpu::init(rs);
            adapter_name = rs.adapter.get_info().name;
        }
        let (center, height) = Kind::Mandelbrot.default_view();
        let view = View { center, height };
        Self {
            kind: Kind::Mandelbrot,
            view,
            target: view,
            julia_c: JULIA_PRESETS[0].c,
            rotate_c: false,
            rotate_phase: 0.0,
            power: 2.0,
            auto_iter: true,
            max_iter: 300,
            ssaa: 2,
            light_while_moving: true,
            palette: 0,
            color_scale: 1.0,
            color_offset: 0.0,
            animate_colors: false,
            anim_phase: 0.0,
            show_panel: true,
            cursor_c: None,
            at_precision_limit: false,
            deep_mode: false,
            adapter_name,
        }
    }

    fn select_kind(&mut self, kind: Kind) {
        self.kind = kind;
        self.power = 2.0;
        match kind {
            Kind::Julia => self.julia_c = JULIA_PRESETS[0].c,
            Kind::Phoenix => self.julia_c = fractal::PHOENIX_C,
            _ => {}
        }
        self.reset_view();
    }

    fn reset_view(&mut self) {
        let (center, height) = self.kind.default_view();
        self.target = View { center, height };
    }

    fn go_to(&mut self, center: [f64; 2], height: f64) {
        self.target = View { center, height };
    }

    fn default_height(&self) -> f64 {
        self.kind.default_view().1
    }

    fn zoom_factor(&self) -> f64 {
        self.default_height() / self.view.height
    }

    fn effective_iterations(&self) -> u32 {
        if self.auto_iter {
            let z = self.zoom_factor().max(1.0);
            (100.0 + 60.0 * z.log2()).clamp(100.0, 10_000.0) as u32
        } else {
            self.max_iter
        }
    }

    /// Mandelbrot, Julia and Burning Ship (power 2) support deep zoom via perturbation.
    fn deep_capable(&self) -> bool {
        matches!(self.kind, Kind::Mandelbrot | Kind::Julia | Kind::BurningShip) && self.power == 2.0
    }

    /// Minimum view height for an area of `height_px` pixels.
    fn min_height(&self, height_px: f64) -> f64 {
        let mag = self.target.center[0].abs().max(self.target.center[1].abs());
        let step = if self.deep_capable() {
            MIN_PIXEL_STEP_DEEP * mag.max(1e-3)
        } else {
            MIN_PIXEL_STEP * mag.max(1.0)
        };
        step * height_px.max(1.0)
    }

    /// Reference orbit in f64 (rounded to f32 for the GPU), computed at the view center.
    fn reference_orbit(&self, max_iter: u32) -> Vec<[f32; 2]> {
        let (mut z, c) = match self.kind {
            Kind::Julia => (self.view.center, self.julia_c),
            _ => ([0.0, 0.0], self.view.center),
        };
        let cap = (max_iter as usize + 1).min(gpu::ORBIT_CAP);
        let mut orbit = Vec::with_capacity(cap);
        orbit.push([z[0] as f32, z[1] as f32]);
        let ship = self.kind == Kind::BurningShip;
        while orbit.len() < cap {
            let xy = z[0] * z[1];
            z = [z[0] * z[0] - z[1] * z[1] + c[0], 2.0 * if ship { xy.abs() } else { xy } + c[1]];
            orbit.push([z[0] as f32, z[1] as f32]);
            if z[0] * z[0] + z[1] * z[1] > ORBIT_BAILOUT_SQ {
                break;
            }
        }
        orbit
    }

    /// Zooms around a screen point (egui points, relative to the area center).
    fn zoom_about(&mut self, offset: Vec2, rect_h: f32, factor: f64, height_px: f64) {
        let t = self.target;
        let upp = t.height / rect_h as f64;
        let wx = t.center[0] + offset.x as f64 * upp;
        let wy = t.center[1] - offset.y as f64 * upp;
        let new_h = (t.height / factor).clamp(self.min_height(height_px), 8.0);
        let new_upp = new_h / rect_h as f64;
        self.target = View {
            center: [wx - offset.x as f64 * new_upp, wy + offset.y as f64 * new_upp],
            height: new_h,
        };
    }
}

impl eframe::App for App {
    fn ui(&mut self, ui: &mut egui::Ui, _frame: &mut eframe::Frame) {
        let ctx = ui.ctx().clone();
        let dt = ctx.input(|i| i.stable_dt).min(0.1);

        // Keyboard shortcuts.
        if ctx.input(|i| i.key_pressed(egui::Key::Tab)) {
            self.show_panel = !self.show_panel;
        }
        if ctx.input(|i| i.key_pressed(egui::Key::R)) {
            self.reset_view();
        }
        if ctx.input(|i| i.key_pressed(egui::Key::F11) || i.key_pressed(egui::Key::F)) {
            let full = ctx.input(|i| i.viewport().fullscreen.unwrap_or(false));
            ctx.send_viewport_cmd(egui::ViewportCommand::Fullscreen(!full));
        }

        // Parameter animations.
        if self.animate_colors {
            self.anim_phase += dt * 0.08;
            ctx.request_repaint();
        }
        if self.rotate_c && self.kind == Kind::Julia {
            self.rotate_phase += dt as f64 * 0.25;
            let r = (self.julia_c[0].powi(2) + self.julia_c[1].powi(2)).sqrt();
            self.julia_c = [r * self.rotate_phase.cos(), r * self.rotate_phase.sin()];
            ctx.request_repaint();
        }

        // Smooth the camera toward the target.
        let dh = self.target.height.ln() - self.view.height.ln();
        let dcx = self.target.center[0] - self.view.center[0];
        let dcy = self.target.center[1] - self.view.center[1];
        let cam_moving = dh.abs() > 1e-4 || dcx.abs().max(dcy.abs()) > 1e-4 * self.view.height;
        if cam_moving {
            let a = 1.0 - (-(dt as f64) * 16.0).exp();
            self.view.height = (self.view.height.ln() + dh * a).exp();
            self.view.center[0] += dcx * a;
            self.view.center[1] += dcy * a;
            ctx.request_repaint();
        } else {
            self.view = self.target;
        }

        if self.show_panel {
            egui::Panel::left("controls")
                .resizable(false)
                .exact_size(272.0)
                .frame(
                    egui::Frame::new()
                        .fill(BG_PANEL)
                        .stroke(Stroke::new(1.0, BORDER))
                        .inner_margin(Margin::symmetric(14, 12)),
                )
                .show(ui, |ui| self.controls(ui));
        }

        egui::Panel::bottom("status")
            .resizable(false)
            .exact_size(26.0)
            .frame(
                egui::Frame::new()
                    .fill(BG_PANEL)
                    .stroke(Stroke::new(1.0, BORDER))
                    .inner_margin(Margin::symmetric(12, 4)),
            )
            .show(ui, |ui| self.status_bar(ui));

        egui::CentralPanel::default()
            .frame(egui::Frame::new().fill(BG_EDITOR))
            .show(ui, |ui| self.canvas(ui, cam_moving));
    }
}

impl App {
    fn canvas(&mut self, ui: &mut egui::Ui, cam_moving: bool) {
        let ctx = ui.ctx().clone();
        let size = ui.available_size();
        let (rect, resp) = ui.allocate_exact_size(size, Sense::click_and_drag());
        let ppp = ctx.pixels_per_point();
        let rect_h = rect.height().max(1.0);
        let height_px = (rect_h * ppp) as f64;

        // Respect the precision limit.
        let min_h = self.min_height(height_px);
        self.target.height = self.target.height.max(min_h);
        self.view.height = self.view.height.max(min_h);

        // Pan: applied immediately to both view and target.
        if resp.dragged() {
            let d = resp.drag_delta();
            let upp = self.view.height / rect_h as f64;
            let (dx, dy) = (-(d.x as f64) * upp, d.y as f64 * upp);
            self.view.center[0] += dx;
            self.view.center[1] += dy;
            self.target.center[0] += dx;
            self.target.center[1] += dy;
        }

        // Wheel / pinch zoom, centered on the cursor.
        let hover = resp.hover_pos();
        if let Some(pos) = hover {
            let (scroll, pinch) = ctx.input(|i| (i.smooth_scroll_delta.y, i.zoom_delta()));
            let factor = (scroll as f64 * 0.0025).exp() * pinch as f64;
            if (factor - 1.0).abs() > 1e-6 {
                self.zoom_about(pos - rect.center(), rect_h, factor, height_px);
            }
            if resp.double_clicked() {
                self.zoom_about(pos - rect.center(), rect_h, 2.5, height_px);
            } else if resp.secondary_clicked() {
                self.zoom_about(pos - rect.center(), rect_h, 0.4, height_px);
            }
            let off = pos - rect.center();
            let upp = self.view.height / rect_h as f64;
            self.cursor_c = Some([
                self.view.center[0] + off.x as f64 * upp,
                self.view.center[1] - off.y as f64 * upp,
            ]);
        } else {
            self.cursor_c = None;
        }

        self.at_precision_limit = self.target.height <= self.min_height(height_px) * 1.001;

        let moving = cam_moving || resp.dragged();
        let ssaa = if moving && self.light_while_moving { 1 } else { self.ssaa };

        let max_iter = self.effective_iterations();
        self.deep_mode = self.deep_capable() && self.view.height / height_px < DEEP_THRESHOLD;
        let orbit = if self.deep_mode { self.reference_orbit(max_iter) } else { Vec::new() };

        let uniforms = gpu::Uniforms {
            center: [self.view.center[0] as f32, self.view.center[1] as f32],
            resolution: [rect.width() * ppp, rect.height() * ppp],
            julia_c: [self.julia_c[0] as f32, self.julia_c[1] as f32],
            scale: (self.view.height / height_px) as f32,
            power: self.power,
            max_iter,
            kind: self.kind as u32,
            palette: self.palette as u32,
            ssaa,
            color_scale: self.color_scale,
            color_offset: self.color_offset + self.anim_phase,
            linear_out: 0,
            orbit_len: orbit.len() as u32,
        };
        ui.painter().add(gpu::paint_callback(rect, uniforms, orbit));

        if moving {
            ctx.request_repaint();
        }
    }

    fn status_bar(&self, ui: &mut egui::Ui) {
        ui.horizontal_centered(|ui| {
            ui.spacing_mut().item_spacing.x = 18.0;
            ui.label(RichText::new(self.kind.name()).color(TEXT).size(12.0));
            ui.label(RichText::new(format!("Zoom ×{:.2e}", self.zoom_factor())).color(TEXT_MUTED).size(12.0));
            ui.label(RichText::new(format!("{} iterations", self.effective_iterations())).color(TEXT_MUTED).size(12.0));
            if let Some(c) = self.cursor_c {
                let dec = ((-(self.view.height / 1000.0).log10()).ceil() as i32 + 1).clamp(3, 16) as usize;
                let sign = if c[1] < 0.0 { '−' } else { '+' };
                ui.label(
                    RichText::new(format!("{:.dec$} {sign} {:.dec$}i", c[0], c[1].abs()))
                        .color(TEXT_MUTED)
                        .size(12.0)
                        .monospace(),
                );
            }
            if self.deep_mode {
                ui.label(RichText::new("Deep zoom").color(ACCENT).size(12.0));
            }
            if self.at_precision_limit {
                ui.label(RichText::new("Precision limit reached").color(Color32::from_rgb(0xe0, 0xb0, 0x5a)).size(12.0));
            }
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                ui.label(RichText::new(&self.adapter_name).color(TEXT_MUTED).size(12.0));
            });
        });
    }

    fn controls(&mut self, ui: &mut egui::Ui) {
        egui::ScrollArea::vertical().auto_shrink([false, false]).show(ui, |ui| {
            ui.set_width(ui.available_width());

            section(ui, "Fractal");
            for k in Kind::ALL {
                if ui.selectable_label(self.kind == k, k.name()).clicked() && self.kind != k {
                    self.select_kind(k);
                }
            }

            if self.kind == Kind::Mandelbrot {
                section(ui, "Famous places");
                for p in MANDELBROT_PLACES {
                    let here = self.target.center == p.center && self.target.height == p.height;
                    if ui.selectable_label(here, p.name).clicked() {
                        self.power = 2.0;
                        self.go_to(p.center, p.height);
                    }
                }
            }

            if self.kind.has_c() || self.kind.has_power() {
                section(ui, "Parameters");
            }
            if self.kind == Kind::Julia {
                let current = JULIA_PRESETS.iter().find(|p| p.c == self.julia_c);
                egui::ComboBox::from_id_salt("julia_presets")
                    .width(ui.available_width())
                    .selected_text(current.map_or("Famous constants", |p| p.name))
                    .show_ui(ui, |ui| {
                        for p in JULIA_PRESETS {
                            if ui.selectable_label(current.is_some_and(|q| q.name == p.name), p.name).clicked() {
                                self.julia_c = p.c;
                                self.rotate_c = false;
                            }
                        }
                    });
                ui.add_space(4.0);
            }
            if self.kind.has_c() {
                let (a, b) = if self.kind == Kind::Phoenix { ("cx", "p") } else { ("Re c", "Im c") };
                param_slider(ui, a, &mut self.julia_c[0], -2.0..=2.0, 4);
                param_slider(ui, b, &mut self.julia_c[1], -2.0..=2.0, 4);
                if self.kind == Kind::Julia {
                    ui.checkbox(&mut self.rotate_c, "Rotate c");
                }
            }
            if self.kind.has_power() {
                ui.horizontal(|ui| {
                    ui.label(RichText::new("Exponent").color(TEXT_MUTED));
                    ui.add_space(4.0);
                    ui.add(
                        egui::Slider::new(&mut self.power, 2.0..=8.0)
                            .step_by(0.01)
                            .fixed_decimals(2)
                            .trailing_fill(true),
                    );
                });
            }

            section(ui, "Rendering");
            ui.checkbox(&mut self.auto_iter, "Automatic iterations");
            if !self.auto_iter {
                ui.add(egui::Slider::new(&mut self.max_iter, 32..=20_000).logarithmic(true).text("iter."));
            }
            ui.add_space(2.0);
            ui.horizontal(|ui| {
                ui.selectable_value(&mut self.ssaa, 1, "Fast");
                ui.selectable_value(&mut self.ssaa, 2, "Smooth 2×");
                ui.selectable_value(&mut self.ssaa, 3, "Ultra 3×");
            });
            ui.checkbox(&mut self.light_while_moving, "Lighten while moving");

            section(ui, "Colors");
            for (i, (name, _)) in PALETTES.iter().enumerate() {
                if palette_row(ui, i, name, self.palette == i).clicked() {
                    self.palette = i;
                }
            }
            ui.add_space(6.0);
            ui.horizontal(|ui| {
                ui.label(RichText::new("Density").color(TEXT_MUTED));
                ui.add(egui::Slider::new(&mut self.color_scale, 0.1..=10.0).logarithmic(true));
            });
            ui.horizontal(|ui| {
                ui.label(RichText::new("Offset").color(TEXT_MUTED));
                ui.add(egui::Slider::new(&mut self.color_offset, 0.0..=1.0));
            });
            ui.checkbox(&mut self.animate_colors, "Color cycling");

            section(ui, "View");
            if ui.button("Reset view").clicked() {
                self.reset_view();
            }
            ui.add_space(10.0);
            ui.label(
                RichText::new(
                    "Drag: pan\nWheel: zoom\nDouble-click: zoom in\nRight-click: zoom out\n\
                     R: reset · Tab: panel · F: fullscreen",
                )
                .size(11.5)
                .color(TEXT_MUTED),
            );
        });
    }
}

fn section(ui: &mut egui::Ui, title: &str) {
    ui.add_space(14.0);
    ui.label(RichText::new(title.to_uppercase()).size(10.5).strong().color(TEXT_MUTED));
    ui.add_space(4.0);
}

fn param_slider(ui: &mut egui::Ui, label: &str, value: &mut f64, range: std::ops::RangeInclusive<f64>, decimals: usize) {
    ui.horizontal(|ui| {
        ui.label(RichText::new(label).color(TEXT_MUTED).monospace());
        ui.add(egui::Slider::new(value, range).fixed_decimals(decimals).trailing_fill(true));
    });
}

/// A palette row: name + gradient, highlighted when selected.
fn palette_row(ui: &mut egui::Ui, idx: usize, name: &str, selected: bool) -> egui::Response {
    let (rect, resp) = ui.allocate_exact_size(Vec2::new(ui.available_width(), 26.0), Sense::click());
    let painter = ui.painter();
    if selected {
        painter.rect_filled(rect, CornerRadius::same(5), ACCENT.gamma_multiply(0.18));
    } else if resp.hovered() {
        painter.rect_filled(rect, CornerRadius::same(5), BG_ELEVATED);
    }
    painter.text(
        rect.left_center() + Vec2::new(8.0, 0.0),
        egui::Align2::LEFT_CENTER,
        name,
        egui::FontId::proportional(13.0),
        if selected { TEXT } else { TEXT_MUTED },
    );
    let bar = egui::Rect::from_min_max(
        egui::pos2(rect.left() + 104.0, rect.top() + 7.0),
        egui::pos2(rect.right() - 8.0, rect.bottom() - 7.0),
    );
    const STEPS: usize = 48;
    let w = bar.width() / STEPS as f32;
    for s in 0..STEPS {
        let t = s as f32 / (STEPS - 1) as f32;
        let [r, g, b] = fractal::palette_color(idx, t);
        let x0 = bar.left() + s as f32 * w;
        let seg = egui::Rect::from_min_max(egui::pos2(x0, bar.top()), egui::pos2(x0 + w + 0.5, bar.bottom()));
        painter.rect_filled(seg, CornerRadius::ZERO, Color32::from_rgb(r, g, b));
    }
    resp
}

fn setup_style(ctx: &egui::Context) {
    let mut style = (*ctx.global_style()).clone();
    style.visuals = egui::Visuals::dark();
    let v = &mut style.visuals;
    v.panel_fill = BG_PANEL;
    v.window_fill = BG_PANEL;
    v.extreme_bg_color = BG_EDITOR;
    v.faint_bg_color = BG_PANEL;
    v.override_text_color = Some(TEXT);
    v.selection.bg_fill = ACCENT.gamma_multiply(0.30);
    v.selection.stroke = Stroke::new(1.0, ACCENT);
    v.hyperlink_color = ACCENT;
    v.window_stroke = Stroke::new(1.0, BORDER);
    v.window_corner_radius = CornerRadius::same(8);
    v.menu_corner_radius = CornerRadius::same(8);

    let r = CornerRadius::same(5);
    let w = &mut v.widgets;
    w.noninteractive.bg_stroke = Stroke::new(1.0, BORDER);
    w.noninteractive.fg_stroke = Stroke::new(1.0, TEXT);
    w.inactive.bg_fill = BG_ELEVATED;
    w.inactive.weak_bg_fill = Color32::TRANSPARENT;
    w.inactive.bg_stroke = Stroke::NONE;
    w.inactive.fg_stroke = Stroke::new(1.0, TEXT);
    w.hovered.bg_fill = Color32::from_rgb(0x45, 0x4a, 0x56);
    w.hovered.weak_bg_fill = BG_ELEVATED;
    w.hovered.bg_stroke = Stroke::NONE;
    w.hovered.fg_stroke = Stroke::new(1.0, Color32::WHITE);
    w.active.bg_fill = ACCENT.gamma_multiply(0.8);
    w.active.weak_bg_fill = Color32::from_rgb(0x45, 0x4a, 0x56);
    w.active.bg_stroke = Stroke::NONE;
    w.active.fg_stroke = Stroke::new(1.0, Color32::WHITE);
    w.open.bg_fill = BG_ELEVATED;
    w.open.weak_bg_fill = BG_ELEVATED;
    for ws in [&mut w.noninteractive, &mut w.inactive, &mut w.hovered, &mut w.active, &mut w.open] {
        ws.corner_radius = r;
    }

    style.spacing.item_spacing = Vec2::new(8.0, 5.0);
    style.spacing.button_padding = Vec2::new(8.0, 4.0);
    style.spacing.slider_width = 130.0;
    style.spacing.interact_size.y = 24.0;
    style.spacing.scroll.bar_width = 6.0;

    use egui::{FontId, TextStyle};
    style.text_styles.insert(TextStyle::Body, FontId::proportional(13.0));
    style.text_styles.insert(TextStyle::Button, FontId::proportional(13.0));
    style.text_styles.insert(TextStyle::Small, FontId::proportional(11.0));
    style.text_styles.insert(TextStyle::Monospace, FontId::monospace(12.0));
    ctx.set_global_style(style);
}
