//! The drawing screen: canvas, top and bottom bars and the popup panels, laid out like ibisPaint.
//!
//! The document lives in a photocraft-engine [`Session`]; every edit is an engine command, so
//! undo, layers, selections, filters and the file format are PhotoCraft's own. The canvas shows
//! the composited document, drawn off the UI thread where threads exist (the browser preview
//! renders inline).

use std::sync::Arc;
#[cfg(not(target_arch = "wasm32"))]
use std::sync::mpsc;

use egui::{Align2, Color32, ColorImage, Context, FontId, Id, Margin, Pos2, Rect, Sense, Shape, Stroke, TextureHandle, TextureOptions, Ui, Vec2, pos2, vec2};
use photocraft_doc::Document;
use photocraft_engine::Session;
use serde_json::{Value, json};

use crate::brushes::{self, BRUSHES, BrushDef, Category};
use crate::exporter::{self, Format};
use crate::i18n::{self, t};
use crate::icons::{self, Icon};
use crate::store::Store;
use crate::ui::{self, ACCENT, CANVAS_BG, PANEL, PANEL_DARK, PANEL_DEEP, SIDEBAR, TEXT, TEXT_DIM, toast};

pub enum Exit {
    Gallery,
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Tool {
    Brush,
    Eraser,
    Smudge,
    Blur,
    Bucket,
    Pipette,
    Text,
    SelRect,
    SelEllipse,
    SelLasso,
    Wand,
    Move,
    Line,
    Rect,
    Ellipse,
}

impl Tool {
    fn paints(self) -> bool {
        matches!(self, Tool::Brush | Tool::Eraser | Tool::Smudge | Tool::Blur)
    }
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum Panel {
    None,
    Tools,
    Brushes,
    Color,
    Layers,
    Menu,
    Ruler,
    Filters,
    Canvas,
    Shapes,
    Selection,
    Export,
    Help,
    Gesture,
    Blend,
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum Ruler {
    Off,
    Line,
    MirrorV,
    MirrorH,
    Quad,
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum ColorTab {
    Palette,
    Rgb,
    Hsb,
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum WheelPart {
    Ring,
    Square,
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum FilterKind {
    Gaussian,
    Sharpen,
    Noise,
    BrightnessContrast,
    Invert,
    Grayscale,
}

/// A stroke being drawn (document coordinates).
struct Drag {
    tool: Tool,
    pts: Vec<Pos2>,
    start: Pos2,
    travelled: f32,
    cancelled: bool,
}

/// A finished stroke waiting for the composite that contains it, drawn meanwhile.
struct Pending {
    pts: Vec<Pos2>,
    width: f32,
    color: Color32,
    rev: u64,
}

#[derive(Clone)]
pub struct Frame {
    pub rev: u64,
    pub width: usize,
    pub height: usize,
    pub rgba: Arc<Vec<u8>>,
}

fn render_frame(rev: u64, doc: &Document) -> Frame {
    let rect = photocraft_geom::Rect { x0: 0, y0: 0, x1: doc.size.width as i32, y1: doc.size.height as i32 };
    let max_side = if cfg!(target_arch = "wasm32") { 2048 } else { 4096 };
    let longest = doc.size.width.max(doc.size.height);
    let img = if longest > max_side {
        let k = max_side as f32 / longest as f32;
        photocraft_compose::render_reduced(doc, ((doc.size.width as f32 * k) as u32).max(1), ((doc.size.height as f32 * k) as u32).max(1)).to_rgba8()
    } else {
        photocraft_compose::render(doc, rect).to_rgba8()
    };
    Frame { rev, width: img.width as usize, height: img.height as usize, rgba: Arc::new(img.pixels) }
}

/// Composites the document off the UI thread (inline in the browser).
struct Renderer {
    #[cfg(not(target_arch = "wasm32"))]
    jobs: mpsc::Sender<(u64, Arc<Document>)>,
    #[cfg(not(target_arch = "wasm32"))]
    frames: mpsc::Receiver<Frame>,
    #[cfg(target_arch = "wasm32")]
    queued: Option<(u64, Arc<Document>)>,
}

impl Renderer {
    #[cfg(not(target_arch = "wasm32"))]
    fn new(ctx: &Context) -> Renderer {
        let (jobs, job_rx) = mpsc::channel::<(u64, Arc<Document>)>();
        let (frame_tx, frames) = mpsc::channel::<Frame>();
        let ctx = ctx.clone();
        std::thread::spawn(move || {
            while let Ok(mut job) = job_rx.recv() {
                while let Ok(newer) = job_rx.try_recv() {
                    job = newer;
                }
                let frame = render_frame(job.0, &job.1);
                if frame_tx.send(frame).is_err() {
                    break;
                }
                ctx.request_repaint();
            }
        });
        Renderer { jobs, frames }
    }

    #[cfg(target_arch = "wasm32")]
    fn new(_ctx: &Context) -> Renderer {
        Renderer { queued: None }
    }

    #[cfg(not(target_arch = "wasm32"))]
    fn request(&mut self, rev: u64, doc: Arc<Document>) {
        let _ = self.jobs.send((rev, doc));
    }

    #[cfg(target_arch = "wasm32")]
    fn request(&mut self, rev: u64, doc: Arc<Document>) {
        self.queued = Some((rev, doc));
    }

    #[cfg(not(target_arch = "wasm32"))]
    fn poll(&mut self) -> Option<Frame> {
        let mut last = None;
        while let Ok(f) = self.frames.try_recv() {
            last = Some(f);
        }
        last
    }

    #[cfg(target_arch = "wasm32")]
    fn poll(&mut self) -> Option<Frame> {
        self.queued.take().map(|(rev, doc)| render_frame(rev, &doc))
    }
}

pub struct Editor {
    pub id: String,
    pub title: String,
    session: Session,
    store: Store,
    renderer: Renderer,
    tex: Option<TextureHandle>,
    shown: Option<Frame>,
    requested_rev: Option<u64>,

    tool: Tool,
    brush: usize,
    tweaks: Vec<[f32; 3]>,
    brush_open: Option<usize>,
    brush_cat: usize,
    sizes: [f32; 4],
    opacity: f32,
    hsv: [f32; 3],
    hex: String,
    recent: Vec<[u8; 3]>,
    color_tab: ColorTab,
    wheel: Option<WheelPart>,

    pan: Vec2,
    zoom: f32,
    fitted: bool,
    drag: Option<Drag>,
    suppress: bool,
    panning: bool,
    prev_tool: Tool,
    placing: bool,
    pending: Vec<Pending>,
    finger_draws: bool,
    stabilizer: f32,
    ruler: Ruler,
    show_sliders: bool,
    immersive: bool,
    canvas_bg: usize,

    panel: Panel,
    filter: Option<FilterKind>,
    filter_a: f32,
    filter_b: f32,
    rename: Option<(u64, String)>,
    text_dialog: Option<(Pos2, String)>,
    export_format: Format,
    export_share: bool,
    leave: bool,
    fresh_panel: bool,

    last_saved_rev: u64,
    last_change_time: f64,
    drawing_secs: f64,
    drawn_unsaved: f64,
    last_frame_time: f64,
    saving: Option<SaveJob>,
    sel_outline: Option<Vec<Pos2>>,
}

#[cfg(not(target_arch = "wasm32"))]
type SaveJob = std::thread::JoinHandle<Result<(), String>>;
#[cfg(target_arch = "wasm32")]
type SaveJob = ();

fn hsv_to_rgb(h: f32, s: f32, v: f32) -> [f32; 3] {
    let h = (h.fract() + 1.0).fract() * 6.0;
    let i = h.floor();
    let f = h - i;
    let (p, q, t) = (v * (1.0 - s), v * (1.0 - s * f), v * (1.0 - s * (1.0 - f)));
    match i as i32 {
        0 => [v, t, p],
        1 => [q, v, p],
        2 => [p, v, t],
        3 => [p, q, v],
        4 => [t, p, v],
        _ => [v, p, q],
    }
}

fn rgb_to_hsv(r: f32, g: f32, b: f32) -> [f32; 3] {
    let max = r.max(g).max(b);
    let min = r.min(g).min(b);
    let d = max - min;
    let h = if d == 0.0 {
        0.0
    } else if max == r {
        ((g - b) / d).rem_euclid(6.0) / 6.0
    } else if max == g {
        ((b - r) / d + 2.0) / 6.0
    } else {
        ((r - g) / d + 4.0) / 6.0
    };
    [h, if max == 0.0 { 0.0 } else { d / max }, max]
}

const PALETTE: [[u8; 3]; 24] = [
    [0, 0, 0],
    [64, 64, 64],
    [128, 128, 128],
    [192, 192, 192],
    [255, 255, 255],
    [120, 72, 40],
    [229, 57, 53],
    [244, 143, 177],
    [255, 112, 67],
    [255, 183, 77],
    [255, 235, 59],
    [205, 220, 57],
    [102, 187, 106],
    [46, 125, 50],
    [38, 166, 154],
    [79, 195, 247],
    [30, 136, 229],
    [26, 35, 126],
    [126, 87, 194],
    [171, 71, 188],
    [236, 64, 122],
    [255, 205, 210],
    [255, 224, 178],
    [200, 230, 201],
];

impl Editor {
    pub fn open(ctx: &Context, store: Store, id: &str) -> Result<Editor, String> {
        let doc = store.load(id)?;
        let meta = store.meta(id).unwrap_or_default();
        let mut session = Session::new();
        session.add_document(doc, None);
        let tweaks = BRUSHES.iter().map(|b| [b.hardness, b.flow, b.spacing]).collect();
        let hsv = [0.0, 0.0, 0.0];
        let mut ed = Editor {
            id: id.to_string(),
            title: meta.title,
            session,
            store,
            renderer: Renderer::new(ctx),
            tex: None,
            shown: None,
            requested_rev: None,
            tool: Tool::Brush,
            brush: 3,
            tweaks,
            brush_open: None,
            brush_cat: 0,
            sizes: [BRUSHES[3].size, 40.0, 40.0, 40.0],
            opacity: 1.0,
            hsv,
            hex: "#000000".into(),
            recent: Vec::new(),
            color_tab: ColorTab::Hsb,
            wheel: None,
            pan: Vec2::ZERO,
            zoom: 1.0,
            fitted: false,
            drag: None,
            suppress: false,
            panning: false,
            prev_tool: Tool::Brush,
            placing: false,
            pending: Vec::new(),
            finger_draws: true,
            stabilizer: 0.3,
            ruler: Ruler::Off,
            show_sliders: true,
            immersive: false,
            canvas_bg: 1,
            panel: Panel::None,
            filter: None,
            filter_a: 5.0,
            filter_b: 0.0,
            rename: None,
            text_dialog: None,
            export_format: Format::Png,
            export_share: false,
            leave: false,
            fresh_panel: false,
            last_saved_rev: 0,
            last_change_time: 0.0,
            drawing_secs: 0.0,
            drawn_unsaved: 0.0,
            last_frame_time: 0.0,
            saving: None,
            sel_outline: None,
        };
        ed.last_saved_rev = ed.revision();
        ed.sync_hex();
        Ok(ed)
    }

    // ---- engine glue -----------------------------------------------------------------------

    fn doc(&self) -> Option<Arc<Document>> {
        self.session.active().map(|d| d.doc.clone())
    }

    fn revision(&self) -> u64 {
        self.session.active().map(|d| d.revision).unwrap_or(0)
    }

    fn active_layer(&self) -> Option<u64> {
        self.session.active().and_then(|d| d.active_layer).map(|l| l.0)
    }

    fn can_undo(&self) -> bool {
        self.session.active().is_some_and(|d| d.history.can_undo())
    }

    fn can_redo(&self) -> bool {
        self.session.active().is_some_and(|d| d.history.can_redo())
    }

    fn has_selection(&self) -> bool {
        self.doc().is_some_and(|d| d.selection.is_some())
    }

    /// Run an engine command; failures show as a message.
    fn run(&mut self, id: &str, params: Value) -> bool {
        match self.session.execute(id, params) {
            Ok(_) => true,
            Err(e) => {
                toast(format!("{e}"));
                false
            }
        }
    }

    fn rgb8(&self) -> [u8; 3] {
        let c = hsv_to_rgb(self.hsv[0], self.hsv[1], self.hsv[2]);
        [(c[0] * 255.0 + 0.5) as u8, (c[1] * 255.0 + 0.5) as u8, (c[2] * 255.0 + 0.5) as u8]
    }

    fn color32(&self) -> Color32 {
        let c = self.rgb8();
        Color32::from_rgb(c[0], c[1], c[2])
    }

    fn sync_hex(&mut self) {
        let c = self.rgb8();
        self.hex = format!("#{:02x}{:02x}{:02x}", c[0], c[1], c[2]);
    }

    fn set_rgb(&mut self, c: [u8; 3]) {
        self.hsv = rgb_to_hsv(c[0] as f32 / 255.0, c[1] as f32 / 255.0, c[2] as f32 / 255.0);
        self.sync_hex();
    }

    fn remember_color(&mut self) {
        let c = self.rgb8();
        self.recent.retain(|x| *x != c);
        self.recent.insert(0, c);
        self.recent.truncate(18);
    }

    fn size_slot(&self) -> usize {
        match self.tool {
            Tool::Eraser => 1,
            Tool::Smudge => 2,
            Tool::Blur => 3,
            _ => 0,
        }
    }

    fn size(&self) -> f32 {
        self.sizes[self.size_slot()]
    }

    fn brush_def(&self) -> BrushDef {
        let mut b = BRUSHES[self.brush.min(BRUSHES.len() - 1)];
        if let Some(t) = self.tweaks.get(self.brush) {
            b.hardness = t[0];
            b.flow = t[1];
            b.spacing = t[2];
        }
        b
    }

    fn select_brush(&mut self, i: usize) {
        self.brush = i;
        self.sizes[0] = BRUSHES[i].size;
        self.opacity = BRUSHES[i].opacity;
        self.tool = Tool::Brush;
    }

    fn set_tool(&mut self, tool: Tool) {
        self.tool = tool;
        self.drag = None;
    }

    // ---- view ------------------------------------------------------------------------------

    fn fit(&mut self, area: Rect) {
        let Some(doc) = self.doc() else { return };
        let (w, h) = (doc.size.width as f32, doc.size.height as f32);
        let zoom = ((area.width() * 0.96) / w).min((area.height() * 0.96) / h).max(0.01);
        self.zoom = zoom;
        self.pan = vec2((area.width() - w * zoom) * 0.5, (area.height() - h * zoom) * 0.5);
        self.fitted = true;
    }

    fn to_screen(&self, origin: Pos2, p: Pos2) -> Pos2 {
        origin + self.pan + p.to_vec2() * self.zoom
    }

    fn to_doc(&self, origin: Pos2, p: Pos2) -> Pos2 {
        ((p - origin - self.pan) / self.zoom).to_pos2()
    }
}

// ---- frame ---------------------------------------------------------------------------------

impl Editor {
    pub fn ui(&mut self, ui: &mut Ui) -> Option<Exit> {
        let ctx = ui.ctx().clone();
        let now = ctx.input(|i| i.time);
        self.tick(&ctx, now);
        if !self.immersive {
            self.top_bar(ui);
        }
        self.bottom_bar(ui);
        egui::CentralPanel::default().frame(egui::Frame::NONE.fill(CANVAS_BG)).show(ui, |ui| self.canvas(ui));
        self.popups(&ctx);
        if self.leave {
            self.flush();
            return Some(Exit::Gallery);
        }
        None
    }

    /// The system Back button.
    pub fn back(&mut self) {
        if self.panel != Panel::None {
            self.panel = Panel::None;
        } else if self.text_dialog.is_some() {
            self.text_dialog = None;
        } else {
            self.leave = true;
        }
    }

    fn tick(&mut self, ctx: &Context, now: f64) {
        let dt = (now - self.last_frame_time).clamp(0.0, 0.5);
        self.last_frame_time = now;
        if self.drag.is_some() {
            self.drawn_unsaved += dt;
        }
        let rev = self.revision();
        if self.requested_rev != Some(rev) {
            if let Some(doc) = self.doc() {
                self.renderer.request(rev, doc);
                self.requested_rev = Some(rev);
            }
            self.last_change_time = now;
        }
        if let Some(frame) = self.renderer.poll() {
            let image = ColorImage::from_rgba_unmultiplied([frame.width, frame.height], &frame.rgba);
            match &mut self.tex {
                Some(t) => t.set(image, TextureOptions::LINEAR),
                None => self.tex = Some(ctx.load_texture("paintcraft-canvas", image, TextureOptions::LINEAR)),
            }
            self.pending.retain(|p| p.rev > frame.rev);
            self.shown = Some(frame);
        }
        if !self.has_selection() {
            self.sel_outline = None;
        }
        self.take_placed();
        self.autosave(ctx, now);
    }

    /// A picture chosen with the top-right image button becomes a layer.
    fn take_placed(&mut self) {
        if !self.placing {
            return;
        }
        let picked = crate::platform::get().take_picked();
        if picked.is_empty() {
            return;
        }
        self.placing = false;
        for p in picked {
            let r = photocraft_engine::file_cmds::place_bytes(&mut self.session, &p.name, p.bytes, None, &json!({"smartObject": false}));
            if let Err(e) = r {
                toast(format!("{e}"));
            }
        }
    }

    fn autosave(&mut self, ctx: &Context, now: f64) {
        #[cfg(not(target_arch = "wasm32"))]
        if self.saving.as_ref().is_some_and(|h| h.is_finished()) {
            if let Some(h) = self.saving.take() {
                if let Ok(Err(e)) = h.join() {
                    toast(e);
                }
            }
        }
        let rev = self.revision();
        if rev != self.last_saved_rev {
            ctx.request_repaint_after(std::time::Duration::from_millis(700));
            if self.drag.is_none() && now - self.last_change_time > 2.0 && self.saving.is_none() {
                self.start_save(rev);
            }
        }
    }

    fn take_seconds(&mut self) -> u64 {
        let s = self.drawn_unsaved.floor();
        self.drawn_unsaved -= s;
        s as u64
    }

    fn start_save(&mut self, rev: u64) {
        let Some(doc) = self.doc() else { return };
        let (store, id, secs) = (self.store.clone(), self.id.clone(), self.take_seconds());
        self.last_saved_rev = rev;
        #[cfg(not(target_arch = "wasm32"))]
        {
            self.saving = Some(std::thread::spawn(move || store.save(&id, &doc, secs)));
        }
        #[cfg(target_arch = "wasm32")]
        {
            if let Err(e) = store.save(&id, &doc, secs) {
                toast(e);
            }
        }
    }

    /// Finish any save in flight and write unsaved changes now (leaving the canvas, closing the app).
    pub fn flush(&mut self) {
        #[cfg(not(target_arch = "wasm32"))]
        if let Some(h) = self.saving.take() {
            let _ = h.join();
        }
        let rev = self.revision();
        if rev != self.last_saved_rev || self.drawn_unsaved >= 1.0 {
            if let Some(doc) = self.doc() {
                let secs = self.take_seconds();
                self.last_saved_rev = rev;
                if let Err(e) = self.store.save(&self.id, &doc, secs) {
                    toast(e);
                }
            }
        }
    }

    // ---- bars ------------------------------------------------------------------------------

    fn top_bar(&mut self, ui: &mut Ui) {
        egui::Panel::top("pc_top").exact_size(58.0).frame(egui::Frame::NONE.fill(CANVAS_BG).inner_margin(Margin::symmetric(8, 6))).show(ui, |ui| {
            let rect = ui.max_rect();
            let gap = 6.0;
            let size = ((rect.width() - gap * 7.0) / 8.0).clamp(30.0, 44.0);
            let y = rect.center().y - size * 0.5;
            let disc = Color32::from_gray(0xa4);
            let ink = Color32::from_gray(0xf0);
            let off = Color32::from_gray(0xb0);
            let btn = |ui: &mut Ui, x: f32, icon: Icon, on: bool, id: &str| -> bool {
                let r = Rect::from_min_size(pos2(x, y), vec2(size, size));
                let resp = ui.interact(r, Id::new(id), Sense::click());
                let fill = if on { ACCENT } else { disc };
                ui.painter().circle_filled(r.center(), size * 0.5, if resp.is_pointer_button_down_on() { fill.gamma_multiply(0.8) } else { fill });
                icons::paint(ui.painter(), r.shrink(size * 0.22), icon, ink);
                resp.clicked()
            };
            let (can_undo, can_redo) = (self.can_undo(), self.can_redo());
            let mut x = rect.left();
            for (icon, enabled, id) in [(Icon::Undo, can_undo, "pc_undo"), (Icon::Redo, can_redo, "pc_redo")] {
                let r = Rect::from_min_size(pos2(x, y), vec2(size, size));
                let resp = ui.interact(r, Id::new(id), Sense::click());
                ui.painter().circle_filled(r.center(), size * 0.5, if enabled { disc } else { off.gamma_multiply(0.7) });
                icons::paint(ui.painter(), r.shrink(size * 0.22), icon, if enabled { ink } else { Color32::from_gray(0xd0) });
                if resp.clicked() && enabled {
                    if icon == Icon::Undo {
                        self.session.undo();
                    } else {
                        self.session.redo();
                    }
                }
                x += size + gap;
            }
            let mut x = rect.right() - size;
            let items = [
                (Icon::Image, false, "pc_image"),
                (Icon::Ruler, self.ruler != Ruler::Off, "pc_ruler"),
                (Icon::Shapes, matches!(self.tool, Tool::Line | Tool::Rect | Tool::Ellipse), "pc_shapes"),
                (Icon::Hand, !self.finger_draws, "pc_hand"),
                (Icon::Select, matches!(self.tool, Tool::SelRect | Tool::SelEllipse | Tool::SelLasso | Tool::Wand) || self.has_selection(), "pc_select"),
                (Icon::Sliders, self.show_sliders, "pc_sliders"),
            ];
            for (icon, on, id) in items {
                if btn(ui, x, icon, on, id) {
                    match id {
                        "pc_image" => {
                            self.placing = true;
                            crate::platform::get().pick_picture();
                        }
                        "pc_ruler" => self.toggle_panel(Panel::Ruler),
                        "pc_shapes" => self.toggle_panel(Panel::Shapes),
                        "pc_hand" => self.toggle_panel(Panel::Gesture),
                        "pc_select" => self.toggle_panel(Panel::Selection),
                        _ => self.show_sliders = !self.show_sliders,
                    }
                }
                x -= size + gap;
            }
        });
    }

    fn toggle_panel(&mut self, p: Panel) {
        self.panel = if self.panel == p { Panel::None } else { p };
    }

    fn bottom_bar(&mut self, ui: &mut Ui) {
        egui::Panel::bottom("pc_bottom").exact_size(64.0).frame(egui::Frame::NONE.fill(Color32::from_gray(0xe9))).show(ui, |ui| {
            let rect = ui.max_rect();
            let slot_w = rect.width() / 7.0;
            let slot = |i: usize| Rect::from_min_size(pos2(rect.left() + slot_w * i as f32, rect.top()), vec2(slot_w, rect.height()));
            let ink = Color32::from_gray(0x55);
            let panel_open = matches!(self.panel, Panel::Brushes | Panel::Color | Panel::Layers | Panel::Filters);
            // 0: brush <-> eraser
            let r = slot(0);
            let resp = ui.interact(r, Id::new("pc_b0"), Sense::click());
            icons::paint(ui.painter(), r.shrink2(vec2(slot_w * 0.22, 14.0)), Icon::Swap, ink);
            if resp.clicked() {
                self.set_tool(if self.tool == Tool::Eraser { Tool::Brush } else { Tool::Eraser });
            }
            // 1: current tool; opens the tool menu
            let r = slot(1);
            let resp = ui.interact(r, Id::new("pc_b1"), Sense::click());
            let tool_icon = tool_icon(self.tool);
            icons::paint(ui.painter(), r.shrink2(vec2(slot_w * 0.22, 14.0)), tool_icon, if self.panel == Panel::Tools { ACCENT } else { ink });
            if resp.clicked() {
                self.toggle_panel(Panel::Tools);
            }
            // 2: size box, or close for an open panel
            let r = slot(2);
            let resp = ui.interact(r, Id::new("pc_b2"), Sense::click());
            let box_r = r.shrink2(vec2(slot_w * 0.12, 9.0));
            if panel_open {
                ui.painter().rect_filled(box_r, 6.0, Color32::from_gray(0xd4));
                icons::paint(ui.painter(), box_r.shrink2(vec2(10.0, 6.0)), Icon::Down, ink);
            } else {
                ui.painter().rect_filled(box_r, 4.0, Color32::BLACK);
                ui.painter().text(box_r.center(), Align2::CENTER_CENTER, format!("{:.1}", self.size()), FontId::proportional(15.0), Color32::WHITE);
            }
            if resp.clicked() {
                if panel_open {
                    self.panel = Panel::None;
                } else {
                    self.show_sliders = !self.show_sliders;
                }
            }
            // 3: colour
            let r = slot(3);
            let resp = ui.interact(r, Id::new("pc_b3"), Sense::click());
            let sw = r.shrink2(vec2(slot_w * 0.16, 12.0));
            ui.painter().rect_filled(sw, 5.0, self.color32());
            ui::stroke_rect(ui.painter(), sw, 5.0, Color32::from_gray(0x30), 2.0);
            if resp.clicked() {
                self.toggle_panel(Panel::Color);
            }
            // 4: eyedropper
            let r = slot(4);
            let resp = ui.interact(r, Id::new("pc_b4"), Sense::click());
            icons::paint(ui.painter(), r.shrink2(vec2(slot_w * 0.22, 14.0)), Icon::Pipette, if self.tool == Tool::Pipette { ACCENT } else { ink });
            if resp.clicked() {
                if self.tool == Tool::Pipette {
                    let back = self.prev_tool;
                    self.set_tool(back);
                } else {
                    self.prev_tool = self.tool;
                    self.set_tool(Tool::Pipette);
                }
            }
            // 5: layers
            let r = slot(5);
            let resp = ui.interact(r, Id::new("pc_b5"), Sense::click());
            let count = self.doc().map(|d| d.layers.len()).unwrap_or(0);
            let lr = r.shrink2(vec2(slot_w * 0.2, 14.0));
            icons::paint(ui.painter(), lr, Icon::Layers, if self.panel == Panel::Layers { ACCENT } else { ink });
            ui.painter().text(lr.center() + vec2(0.0, 1.0), Align2::CENTER_CENTER, count.to_string(), FontId::proportional(11.0), ink);
            if resp.clicked() {
                self.toggle_panel(Panel::Layers);
            }
            // 6: menu
            let r = slot(6);
            let resp = ui.interact(r, Id::new("pc_b6"), Sense::click());
            icons::paint(ui.painter(), r.shrink2(vec2(slot_w * 0.22, 14.0)), Icon::Back, ink);
            if resp.clicked() {
                self.toggle_panel(Panel::Menu);
            }
        });
    }
}

fn tool_icon(tool: Tool) -> Icon {
    match tool {
        Tool::Brush => Icon::Brush,
        Tool::Eraser => Icon::Eraser,
        Tool::Smudge => Icon::Smudge,
        Tool::Blur => Icon::Blur,
        Tool::Bucket => Icon::Bucket,
        Tool::Pipette => Icon::Pipette,
        Tool::Text => Icon::Text,
        Tool::SelRect | Tool::SelEllipse => Icon::Select,
        Tool::SelLasso => Icon::Lasso,
        Tool::Wand => Icon::Wand,
        Tool::Move => Icon::Move,
        Tool::Line | Tool::Rect | Tool::Ellipse => Icon::Shapes,
    }
}

// ---- canvas --------------------------------------------------------------------------------

impl Editor {
    fn canvas(&mut self, ui: &mut Ui) {
        let rect = ui.max_rect();
        let ctx = ui.ctx().clone();
        if !self.fitted {
            self.fit(Rect::from_min_size(Pos2::ZERO, rect.size()));
        }
        let painter = ui.painter_at(rect);
        painter.rect_filled(rect, 0.0, CANVAS_BG);
        if let Some(doc) = self.doc() {
            let size = vec2(doc.size.width as f32, doc.size.height as f32) * self.zoom;
            let dr = Rect::from_min_size(rect.min + self.pan, size);
            painter.rect_filled(dr.expand(2.0).translate(vec2(0.0, 2.0)), 2.0, Color32::from_black_alpha(48));
            let vis = dr.intersect(rect);
            if vis.is_positive() {
                match self.canvas_bg {
                    0 => {
                        painter.rect_filled(dr, 0.0, Color32::WHITE);
                    }
                    1 => ui::checker(&painter, vis, 14.0),
                    2 => {
                        painter.rect_filled(dr, 0.0, Color32::from_gray(0x38));
                    }
                    _ => {}
                }
            }
            if let Some(tex) = &self.tex {
                painter.image(tex.id(), dr, Rect::from_min_max(pos2(0.0, 0.0), pos2(1.0, 1.0)), Color32::WHITE);
            }
        }
        self.handle_input(ui, &ctx, rect);
        for p in &self.pending {
            self.draw_poly(&painter, rect.min, &p.pts, p.width, p.color);
        }
        self.draw_drag_preview(&painter, rect.min);
        if let Some(outline) = &self.sel_outline {
            let mut pts: Vec<Pos2> = outline.iter().map(|p| self.to_screen(rect.min, *p)).collect();
            if let Some(first) = pts.first().copied() {
                pts.push(first);
            }
            painter.extend(Shape::dashed_line(&pts, Stroke::new(3.0, Color32::BLACK), 7.0, 7.0));
            painter.extend(Shape::dashed_line(&pts, Stroke::new(1.5, Color32::WHITE), 7.0, 7.0));
        }
    }

    fn handle_input(&mut self, ui: &mut Ui, ctx: &Context, rect: Rect) {
        let (down, pressed, pos, multi, scroll, delta) = ui.input(|i| (i.pointer.primary_down(), i.pointer.primary_pressed(), i.pointer.latest_pos(), i.multi_touch(), i.smooth_scroll_delta, i.pointer.delta()));
        let over = pos.is_some_and(|p| rect.contains(p) && ctx.layer_id_at(p) == Some(ui.layer_id()));
        // Two fingers: pan and zoom, and drop any stroke the first finger began.
        if let Some(mt) = multi {
            self.suppress = true;
            self.drag = None;
            let new_zoom = (self.zoom * mt.zoom_delta).clamp(0.02, 64.0);
            let k = new_zoom / self.zoom;
            let a = mt.center_pos - rect.min;
            self.pan = a - (a - self.pan) * k + mt.translation_delta;
            self.zoom = new_zoom;
        } else if over && scroll.y.abs() > 0.0 && self.drag.is_none() {
            let new_zoom = (self.zoom * (scroll.y * 0.0015).exp()).clamp(0.02, 64.0);
            let k = new_zoom / self.zoom;
            if let Some(p) = pos {
                let a = p - rect.min;
                self.pan = a - (a - self.pan) * k;
            }
            self.zoom = new_zoom;
        }
        if pressed && over && multi.is_none() {
            if matches!(self.panel, Panel::Tools | Panel::Menu | Panel::Ruler | Panel::Shapes | Panel::Selection | Panel::Gesture | Panel::Blend | Panel::Canvas | Panel::Export | Panel::Help) {
                self.panel = Panel::None;
                self.suppress = true;
            } else if !self.suppress {
                if self.finger_draws {
                    if let Some(p) = pos {
                        self.begin_drag(self.to_doc(rect.min, p));
                    }
                } else {
                    self.panning = true;
                }
            }
        }
        if down && multi.is_none() {
            if self.panning {
                self.pan += delta;
            }
            let dp = pos.map(|p| self.to_doc(rect.min, p));
            let (zoom, stab) = (self.zoom, self.stabilizer);
            let ruler = self.ruler;
            if let (Some(d), Some(dp)) = (&mut self.drag, dp) {
                let straight = matches!(d.tool, Tool::Line | Tool::Rect | Tool::Ellipse | Tool::SelRect | Tool::SelEllipse | Tool::Move) || (d.tool.paints() && ruler == Ruler::Line);
                d.travelled = d.travelled.max((dp - d.start).length() * zoom);
                if straight {
                    d.pts.truncate(1);
                    d.pts.push(dp);
                } else if let Some(last) = d.pts.last().copied() {
                    let smooth = d.tool.paints();
                    let np = if smooth { last + (dp - last) * (1.0 - stab) } else { dp };
                    if (np - last).length() * zoom > 0.6 {
                        d.pts.push(np);
                    }
                }
            }
        }
        if !down {
            if let Some(d) = self.drag.take() {
                if !d.cancelled && !self.suppress {
                    let end = pos.map(|p| self.to_doc(rect.min, p));
                    self.commit(d, end);
                }
            }
            if multi.is_none() {
                self.suppress = false;
            }
            self.panning = false;
        }
    }

    fn begin_drag(&mut self, p: Pos2) {
        self.drag = Some(Drag { tool: self.tool, pts: vec![p], start: p, travelled: 0.0, cancelled: false });
    }

    fn doc_size(&self) -> (f32, f32) {
        self.doc().map(|d| (d.size.width as f32, d.size.height as f32)).unwrap_or((1.0, 1.0))
    }

    fn draw_poly(&self, painter: &egui::Painter, origin: Pos2, pts: &[Pos2], width: f32, color: Color32) {
        if pts.is_empty() {
            return;
        }
        let w = (width * self.zoom).max(1.0);
        let screen: Vec<Pos2> = pts.iter().map(|p| self.to_screen(origin, *p)).collect();
        if screen.len() == 1 {
            painter.circle_filled(screen[0], w * 0.5, color);
            return;
        }
        painter.add(Shape::line(screen.clone(), Stroke::new(w, color)));
        painter.circle_filled(screen[0], w * 0.5, color);
        if let Some(last) = screen.last() {
            painter.circle_filled(*last, w * 0.5, color);
        }
    }

    /// The point sets a stroke makes with the ruler's symmetry.
    fn mirrored(&self, pts: &[Pos2]) -> Vec<Vec<Pos2>> {
        let (w, h) = self.doc_size();
        let flip_x = |v: &[Pos2]| v.iter().map(|p| pos2(w - p.x, p.y)).collect::<Vec<_>>();
        let flip_y = |v: &[Pos2]| v.iter().map(|p| pos2(p.x, h - p.y)).collect::<Vec<_>>();
        let mut sets = vec![pts.to_vec()];
        match self.ruler {
            Ruler::MirrorV => sets.push(flip_x(pts)),
            Ruler::MirrorH => sets.push(flip_y(pts)),
            Ruler::Quad => {
                let fx = flip_x(pts);
                sets.push(flip_y(pts));
                sets.push(flip_y(&fx));
                sets.push(fx);
            }
            _ => {}
        }
        sets
    }

    fn shape_points(tool: Tool, a: Pos2, b: Pos2) -> Vec<Pos2> {
        match tool {
            Tool::Rect => vec![a, pos2(b.x, a.y), b, pos2(a.x, b.y), a],
            Tool::Ellipse => {
                let (c, r) = (pos2((a.x + b.x) * 0.5, (a.y + b.y) * 0.5), vec2((b.x - a.x).abs() * 0.5, (b.y - a.y).abs() * 0.5));
                (0..=72).map(|i| {
                    let t = i as f32 / 72.0 * std::f32::consts::TAU;
                    pos2(c.x + r.x * t.cos(), c.y + r.y * t.sin())
                }).collect()
            }
            _ => vec![a, b],
        }
    }

    fn draw_drag_preview(&self, painter: &egui::Painter, origin: Pos2) {
        let Some(d) = &self.drag else { return };
        let size = self.size();
        let mut c = self.color32();
        c = Color32::from_rgba_unmultiplied(c.r(), c.g(), c.b(), (self.opacity * 255.0) as u8);
        match d.tool {
            Tool::Brush | Tool::Line | Tool::Rect | Tool::Ellipse => {
                let pts = if matches!(d.tool, Tool::Line | Tool::Rect | Tool::Ellipse) { Self::shape_points(d.tool, d.start, *d.pts.last().unwrap_or(&d.start)) } else { d.pts.clone() };
                for set in self.mirrored(&pts) {
                    self.draw_poly(painter, origin, &set, size, c);
                }
            }
            Tool::Eraser | Tool::Smudge | Tool::Blur => {
                for set in self.mirrored(&d.pts) {
                    self.draw_poly(painter, origin, &set, size, Color32::from_rgba_unmultiplied(255, 255, 255, 90));
                }
            }
            Tool::SelRect | Tool::SelEllipse => {
                let tool = if d.tool == Tool::SelRect { Tool::Rect } else { Tool::Ellipse };
                let pts: Vec<Pos2> = Self::shape_points(tool, d.start, *d.pts.last().unwrap_or(&d.start)).iter().map(|p| self.to_screen(origin, *p)).collect();
                painter.add(Shape::line(pts.clone(), Stroke::new(3.0, Color32::BLACK)));
                painter.add(Shape::line(pts, Stroke::new(1.5, Color32::WHITE)));
            }
            Tool::SelLasso => {
                let pts: Vec<Pos2> = d.pts.iter().map(|p| self.to_screen(origin, *p)).collect();
                painter.add(Shape::line(pts.clone(), Stroke::new(3.0, Color32::BLACK)));
                painter.add(Shape::line(pts, Stroke::new(1.5, Color32::WHITE)));
            }
            Tool::Move => {
                if let Some(last) = d.pts.last() {
                    let (a, b) = (self.to_screen(origin, d.start), self.to_screen(origin, *last));
                    painter.line_segment([a, b], Stroke::new(2.0, ACCENT));
                    painter.circle_filled(b, 6.0, ACCENT);
                }
            }
            _ => {
                let p = self.to_screen(origin, d.start);
                painter.circle_stroke(p, 14.0, Stroke::new(2.0, Color32::BLACK));
                painter.circle_stroke(p, 12.0, Stroke::new(2.0, Color32::WHITE));
            }
        }
    }

    // ---- committing a gesture --------------------------------------------------------------

    fn commit(&mut self, d: Drag, end: Option<Pos2>) {
        let last = end.or_else(|| d.pts.last().copied()).unwrap_or(d.start);
        let tap = d.travelled < 14.0;
        match d.tool {
            Tool::Brush | Tool::Eraser | Tool::Smudge | Tool::Blur => {
                let mut pts = d.pts.clone();
                if d.tool.paints() && self.ruler != Ruler::Line && pts.last().is_some_and(|p| (*p - last).length() > 0.01) {
                    pts.push(last);
                }
                if self.ruler == Ruler::Line {
                    pts = vec![d.start, last];
                }
                self.paint_sets(d.tool, &pts);
            }
            Tool::Line | Tool::Rect | Tool::Ellipse => {
                let pts = Self::shape_points(d.tool, d.start, last);
                self.paint_sets(Tool::Brush, &pts);
            }
            Tool::Bucket if tap => {
                let hex = self.hex.clone();
                let alpha = (self.opacity * 100.0).clamp(1.0, 100.0);
                if self.run("paint.bucket", json!({"x": d.start.x.round(), "y": d.start.y.round(), "tolerance": 32, "contiguous": true, "color": hex, "opacity": alpha})) {
                    self.remember_color();
                }
            }
            Tool::Pipette if tap => {
                self.pick_color(d.start);
                let back = self.prev_tool;
                self.set_tool(back);
            }
            Tool::Text if tap => self.text_dialog = Some((d.start, String::new())),
            Tool::Wand if tap => {
                if self.run("select.magicWand", json!({"x": d.start.x.round(), "y": d.start.y.round(), "tolerance": 32, "contiguous": true})) {
                    self.sel_outline = None;
                }
            }
            Tool::SelRect | Tool::SelEllipse => {
                let (a, b) = (d.start, last);
                let (x, y) = (a.x.min(b.x).round() as i32, a.y.min(b.y).round() as i32);
                let (w, h) = ((a.x - b.x).abs().round() as u32, (a.y - b.y).abs().round() as u32);
                if w > 1 && h > 1 {
                    let ellipse = d.tool == Tool::SelEllipse;
                    if self.run("select.rect", json!({"x": x, "y": y, "width": w, "height": h, "ellipse": ellipse})) {
                        self.sel_outline = Some(Self::shape_points(if ellipse { Tool::Ellipse } else { Tool::Rect }, a, b));
                    }
                }
            }
            Tool::SelLasso => {
                if d.pts.len() >= 3 {
                    let points: Vec<Value> = d.pts.iter().map(|p| json!([p.x, p.y])).collect();
                    if self.run("select.lasso", json!({"points": points})) {
                        self.sel_outline = Some(d.pts.clone());
                    }
                }
            }
            Tool::Move => {
                let (dx, dy) = ((last.x - d.start.x).round() as i32, (last.y - d.start.y).round() as i32);
                if dx != 0 || dy != 0 {
                    self.run("layer.translate", json!({"dx": dx, "dy": dy}));
                }
            }
            _ => {}
        }
    }

    fn paint_sets(&mut self, tool: Tool, pts: &[Pos2]) {
        let sets = self.mirrored(pts);
        let size = self.size();
        for set in sets {
            if set.is_empty() {
                continue;
            }
            let mut points: Vec<Value> = set.iter().map(|p| json!([p.x, p.y, 1.0])).collect();
            if points.len() == 1 {
                points.push(json!([set[0].x + 0.01, set[0].y, 1.0]));
            }
            let def = self.brush_def();
            let hex = self.hex.clone();
            let (id, params) = match tool {
                Tool::Eraser => ("paint.stroke", json!({"points": points, "size": size, "opacity": self.opacity, "erase": true, "brush": {"hardness": 0.9, "spacing": 0.08, "flow": 1.0}})),
                Tool::Smudge => ("paint.smudge", json!({"points": points, "size": size, "strength": (self.opacity * 100.0).clamp(1.0, 100.0)})),
                Tool::Blur => ("paint.blur", json!({"points": points, "size": size, "strength": (self.opacity * 100.0).clamp(1.0, 100.0)})),
                _ => ("paint.stroke", json!({"points": points, "size": size, "opacity": self.opacity, "color": hex, "brush": def.patch()})),
            };
            if self.run(id, params) && tool == Tool::Brush {
                let c = self.color32();
                let rev = self.revision();
                self.pending.push(Pending { pts: set, width: size, color: Color32::from_rgba_unmultiplied(c.r(), c.g(), c.b(), (self.opacity * 255.0) as u8), rev });
            }
        }
        if tool == Tool::Brush {
            self.remember_color();
        }
    }

    fn pick_color(&mut self, p: Pos2) {
        let Some(frame) = self.shown.clone() else { return };
        let (w, h) = self.doc_size();
        let x = ((p.x / w) * frame.width as f32) as usize;
        let y = ((p.y / h) * frame.height as f32) as usize;
        if x >= frame.width || y >= frame.height {
            return;
        }
        let i = (y * frame.width + x) * 4;
        if let Some(px) = frame.rgba.get(i..i + 4) {
            self.set_rgb([px[0], px[1], px[2]]);
        }
    }
}

// ---- popups --------------------------------------------------------------------------------

const BAR_H: f32 = 64.0;

fn chip(ui: &mut Ui, label: &str, selected: bool) -> egui::Response {
    ui::text_button(ui, label, if selected { ACCENT } else { PANEL_DEEP }, Color32::WHITE, 70.0)
}

fn tile(ui: &mut Ui, size: Vec2, icon: Icon, label: &str, selected: bool, enabled: bool) -> egui::Response {
    let (rect, resp) = ui.allocate_exact_size(size, Sense::click());
    let fill = if selected { ACCENT.gamma_multiply(0.55) } else if resp.is_pointer_button_down_on() { Color32::from_white_alpha(30) } else { Color32::TRANSPARENT };
    ui.painter().rect_filled(rect, 10.0, fill);
    let col = if enabled { Color32::WHITE } else { Color32::from_white_alpha(70) };
    let icon_rect = Rect::from_center_size(pos2(rect.center().x, rect.top() + size.y * 0.38), vec2(size.y * 0.46, size.y * 0.46));
    icons::paint(ui.painter(), icon_rect, icon, col);
    ui.painter().text(pos2(rect.center().x, rect.bottom() - size.y * 0.17), Align2::CENTER_CENTER, label, FontId::proportional(12.5), col);
    resp
}

impl Editor {
    fn sheet(&mut self, ctx: &Context, id: &str, height: f32, fill: Color32, content: impl FnOnce(&mut Editor, &mut Ui, f32)) {
        let screen = ctx.content_rect();
        let margin = 6.0;
        let w = screen.width() - margin * 2.0;
        let height = height.min(screen.height() - BAR_H - 90.0).max(240.0);
        let pos = pos2(screen.left() + margin, screen.bottom() - BAR_H - margin - height);
        egui::Area::new(Id::new(id)).order(egui::Order::Foreground).fixed_pos(pos).show(ctx, |ui| {
            ui::card(fill).show(ui, |ui| {
                ui.set_width(w - 20.0);
                ui.set_min_height(height - 20.0);
                ui.set_max_height(height - 20.0);
                content(self, ui, height - 20.0);
            });
        });
    }

    fn popups(&mut self, ctx: &Context) {
        self.quick_sliders(ctx);
        self.selection_chip(ctx);
        match self.panel {
            Panel::None => {}
            Panel::Tools => self.tools_menu(ctx),
            Panel::Brushes => {
                let h = ctx.content_rect().height() * 0.64;
                self.sheet(ctx, "pc_brushes", h, PANEL, |s, ui, h| s.brushes_panel(ui, h));
            }
            Panel::Color => {
                let h = ctx.content_rect().height() * 0.66;
                self.sheet(ctx, "pc_color", h, PANEL, |s, ui, h| s.color_panel(ui, h));
            }
            Panel::Layers => {
                let h = ctx.content_rect().height() * 0.64;
                self.sheet(ctx, "pc_layers", h, PANEL, |s, ui, h| s.layers_panel(ui, h));
            }
            Panel::Filters => {
                let h = ctx.content_rect().height() * 0.5;
                self.sheet(ctx, "pc_filters", h, PANEL, |s, ui, h| s.filters_panel(ui, h));
            }
            Panel::Menu => self.menu_popup(ctx),
            Panel::Ruler => self.sheet(ctx, "pc_ruler_p", 250.0, PANEL, |s, ui, _| s.ruler_panel(ui)),
            Panel::Gesture => self.sheet(ctx, "pc_gesture", 250.0, PANEL, |s, ui, _| s.gesture_panel(ui)),
            Panel::Shapes => self.sheet(ctx, "pc_shapes_p", 170.0, PANEL, |s, ui, _| s.shapes_panel(ui)),
            Panel::Selection => self.sheet(ctx, "pc_sel_p", 300.0, PANEL, |s, ui, _| s.selection_panel(ui)),
            Panel::Canvas => self.sheet(ctx, "pc_canvas_p", 270.0, PANEL, |s, ui, _| s.canvas_panel(ui)),
            Panel::Blend => {
                let h = ctx.content_rect().height() * 0.6;
                self.sheet(ctx, "pc_blend", h, PANEL, |s, ui, h| s.blend_panel(ui, h));
            }
            Panel::Export => self.export_dialog(ctx),
            Panel::Help => self.help_dialog(ctx),
        }
        self.text_and_rename_dialogs(ctx);
    }

    fn quick_sliders(&mut self, ctx: &Context) {
        let wanted = self.show_sliders && !self.immersive && matches!(self.tool, Tool::Brush | Tool::Eraser | Tool::Smudge | Tool::Blur | Tool::Line | Tool::Rect | Tool::Ellipse | Tool::Text | Tool::Bucket);
        if !wanted || !matches!(self.panel, Panel::None | Panel::Tools | Panel::Menu | Panel::Ruler | Panel::Gesture | Panel::Shapes | Panel::Selection | Panel::Canvas | Panel::Blend) {
            return;
        }
        let screen = ctx.content_rect();
        let w = (screen.width() * 0.8).min(420.0);
        let pos = pos2(screen.left() + 8.0, screen.bottom() - BAR_H - 100.0);
        egui::Area::new(Id::new("pc_quick")).order(egui::Order::Foreground).fixed_pos(pos).show(ctx, |ui| {
            egui::Frame::new().fill(Color32::from_black_alpha(90)).corner_radius(14.0).inner_margin(Margin::symmetric(8, 4)).show(ui, |ui| {
                ui.set_width(w);
                let slot = self.size_slot();
                let mut size = self.sizes[slot];
                if ui::stepper_slider(ui, "pc_size", &format!("{size:.1}"), &mut size, 1.0, 500.0, true, 1.0, 1) {
                    self.sizes[slot] = size;
                }
                let mut pct = self.opacity * 100.0;
                if ui::stepper_slider(ui, "pc_opacity", &format!("{pct:.0}"), &mut pct, 1.0, 100.0, false, 1.0, 0) {
                    self.opacity = pct / 100.0;
                }
            });
        });
    }

    fn selection_chip(&mut self, ctx: &Context) {
        if !self.has_selection() || self.immersive || self.panel == Panel::Selection {
            return;
        }
        let screen = ctx.content_rect();
        let top = if self.immersive { 8.0 } else { 66.0 };
        egui::Area::new(Id::new("pc_selchip")).order(egui::Order::Foreground).fixed_pos(pos2(screen.center().x, screen.top() + top)).pivot(Align2::CENTER_TOP).show(ctx, |ui| {
            egui::Frame::new().fill(Color32::from_black_alpha(190)).corner_radius(22.0).inner_margin(Margin::symmetric(8, 4)).show(ui, |ui| {
                ui.horizontal(|ui| {
                    ui.spacing_mut().item_spacing.x = 2.0;
                    let sz = vec2(68.0, 54.0);
                    if tile(ui, sz, Icon::Close, &t("Deselect"), false, true).clicked() {
                        self.run("select.deselect", json!({}));
                    }
                    if tile(ui, sz, Icon::Refresh, &t("Invert"), false, true).clicked() {
                        self.run("select.inverse", json!({}));
                    }
                    if tile(ui, sz, Icon::Bucket, &t("Fill"), false, true).clicked() {
                        self.fill_selection();
                    }
                    if tile(ui, sz, Icon::Trash, &t("Clear"), false, true).clicked() {
                        self.run("edit.clear", json!({}));
                    }
                });
            });
        });
    }

    fn fill_selection(&mut self) {
        let hex = self.hex.clone();
        if self.run("tools.setColors", json!({"foreground": hex})) {
            self.run("edit.fillForeground", json!({}));
        }
    }

    fn tools_menu(&mut self, ctx: &Context) {
        #[derive(Clone, Copy)]
        enum A {
            Tool(Tool),
            Brushes,
            Panel(Panel),
        }
        let items: [(Icon, &str, A); 14] = [
            (Icon::Move, "Transform", A::Tool(Tool::Move)),
            (Icon::Wand, "Magic Wand", A::Tool(Tool::Wand)),
            (Icon::Lasso, "Lasso", A::Tool(Tool::SelLasso)),
            (Icon::Filter, "Filter", A::Panel(Panel::Filters)),
            (Icon::Brush, "Brush", A::Brushes),
            (Icon::Eraser, "Eraser", A::Tool(Tool::Eraser)),
            (Icon::Smudge, "Smudge", A::Tool(Tool::Smudge)),
            (Icon::Blur, "Blur", A::Tool(Tool::Blur)),
            (Icon::Select, "Selection", A::Panel(Panel::Selection)),
            (Icon::Bucket, "Fill", A::Tool(Tool::Bucket)),
            (Icon::Shapes, "Shapes", A::Panel(Panel::Shapes)),
            (Icon::Text, "Text", A::Tool(Tool::Text)),
            (Icon::Pipette, "Eyedropper", A::Tool(Tool::Pipette)),
            (Icon::Canvas, "Canvas", A::Panel(Panel::Canvas)),
        ];
        let screen = ctx.content_rect();
        let pos = pos2(screen.left() + 8.0, screen.bottom() - BAR_H - 8.0);
        let mut chosen = None;
        egui::Area::new(Id::new("pc_tools")).order(egui::Order::Foreground).fixed_pos(pos).pivot(Align2::LEFT_BOTTOM).show(ctx, |ui| {
            egui::Frame::new().fill(Color32::from_rgb(0x6c, 0x6c, 0x6e)).corner_radius(14.0).inner_margin(Margin::same(6)).show(ui, |ui| {
                egui::Grid::new("pc_tools_grid").spacing(vec2(2.0, 2.0)).show(ui, |ui| {
                    for (i, (icon, label, action)) in items.iter().enumerate() {
                        let selected = matches!(action, A::Tool(t) if *t == self.tool);
                        if tile(ui, vec2(112.0, 68.0), *icon, &t(label), selected, true).clicked() {
                            chosen = Some(*action);
                        }
                        if i % 2 == 1 {
                            ui.end_row();
                        }
                    }
                });
            });
        });
        match chosen {
            Some(A::Tool(tool)) => {
                self.set_tool(tool);
                self.panel = Panel::None;
            }
            Some(A::Brushes) => {
                if !matches!(self.tool, Tool::Brush) {
                    self.set_tool(Tool::Brush);
                }
                self.panel = Panel::Brushes;
            }
            Some(A::Panel(p)) => self.panel = p,
            None => {}
        }
    }

    fn menu_popup(&mut self, ctx: &Context) {
        let screen = ctx.content_rect();
        let pos = pos2(screen.right() - 8.0, screen.bottom() - BAR_H - 8.0);
        let mut action = 0;
        egui::Area::new(Id::new("pc_menu")).order(egui::Order::Foreground).fixed_pos(pos).pivot(Align2::RIGHT_BOTTOM).show(ctx, |ui| {
            egui::Frame::new().fill(ui::SHEET).corner_radius(12.0).inner_margin(Margin::same(0)).show(ui, |ui| {
                ui.set_width((screen.width() * 0.72).min(340.0));
                ui.spacing_mut().item_spacing.y = 0.0;
                let rows = [("How to Use", Icon::Info, 1), ("Settings", Icon::Gear, 2), ("Save Image", Icon::Down, 3), ("Share", Icon::Share, 4), ("Back to My Gallery", Icon::Left, 5)];
                for (label, _icon, id) in rows {
                    if ui::row(ui, &t(label), "", false, ui::SHEET, Color32::BLACK).clicked() {
                        action = id;
                    }
                }
            });
        });
        match action {
            1 => self.panel = Panel::Help,
            2 => self.panel = Panel::Gesture,
            3 => {
                self.export_share = false;
                self.panel = Panel::Export;
            }
            4 => {
                self.export_share = true;
                self.panel = Panel::Export;
            }
            5 => {
                self.panel = Panel::None;
                self.leave = true;
            }
            _ => {}
        }
    }

    fn ruler_panel(&mut self, ui: &mut Ui) {
        ui.label(egui::RichText::new(t("Ruler")).size(19.0).color(TEXT).strong());
        ui.add_space(6.0);
        let sz = vec2(78.0, 76.0);
        ui.horizontal(|ui| {
            for (r, icon, label) in [(Ruler::Off, Icon::Close, "Off"), (Ruler::Line, Icon::Ruler, "Straight")] {
                if tile(ui, sz, icon, &t(label), self.ruler == r, true).clicked() {
                    self.ruler = r;
                }
            }
        });
        ui.add_space(8.0);
        ui.label(egui::RichText::new(t("Symmetry Ruler")).size(19.0).color(TEXT).strong());
        ui.add_space(6.0);
        ui.horizontal(|ui| {
            for (r, icon, label) in [(Ruler::MirrorV, Icon::FlipH, "Left-Right"), (Ruler::MirrorH, Icon::FlipV, "Up-Down"), (Ruler::Quad, Icon::Canvas, "4-Way")] {
                if tile(ui, sz, icon, &t(label), self.ruler == r, true).clicked() {
                    self.ruler = r;
                }
            }
        });
    }

    fn gesture_panel(&mut self, ui: &mut Ui) {
        ui.label(egui::RichText::new(t("Touch")).size(19.0).color(TEXT).strong());
        ui.add_space(6.0);
        ui.horizontal(|ui| {
            if chip(ui, &t("Finger draws"), self.finger_draws).clicked() {
                self.finger_draws = true;
            }
            if chip(ui, &t("Finger pans"), !self.finger_draws).clicked() {
                self.finger_draws = false;
            }
        });
        ui.add_space(4.0);
        ui.label(egui::RichText::new(t("Two fingers always pan and zoom.")).color(TEXT_DIM).size(14.0));
        ui.add_space(10.0);
        ui.label(egui::RichText::new(t("Stroke stabilization")).size(17.0).color(TEXT));
        let mut s = self.stabilizer;
        if ui::slider(ui, ui.available_width(), &mut s, 0.0, 0.9, false) {
            self.stabilizer = s;
        }
        ui.add_space(6.0);
        ui.horizontal(|ui| {
            ui.label(egui::RichText::new(t("Language")).size(17.0).color(TEXT));
            for (code, label) in [("auto", "Auto"), ("id", "Bahasa Indonesia"), ("en", "English")] {
                if chip(ui, label, i18n::preference() == code).clicked() {
                    i18n::set_preference(code);
                }
            }
        });
    }

    fn shapes_panel(&mut self, ui: &mut Ui) {
        ui.label(egui::RichText::new(t("Shapes")).size(19.0).color(TEXT).strong());
        ui.add_space(6.0);
        ui.horizontal(|ui| {
            for (tool, icon, label) in [(Tool::Line, Icon::Ruler, "Line"), (Tool::Rect, Icon::Canvas, "Rectangle"), (Tool::Ellipse, Icon::Info, "Ellipse")] {
                if tile(ui, vec2(96.0, 76.0), icon, &t(label), self.tool == tool, true).clicked() {
                    self.set_tool(tool);
                    self.panel = Panel::None;
                }
            }
        });
    }

    fn selection_panel(&mut self, ui: &mut Ui) {
        ui.label(egui::RichText::new(t("Selection")).size(19.0).color(TEXT).strong());
        ui.add_space(6.0);
        ui.horizontal(|ui| {
            for (tool, icon, label) in [(Tool::SelRect, Icon::Select, "Rectangle"), (Tool::SelEllipse, Icon::Info, "Ellipse"), (Tool::SelLasso, Icon::Lasso, "Lasso"), (Tool::Wand, Icon::Wand, "Magic Wand")] {
                if tile(ui, vec2(80.0, 76.0), icon, &t(label), self.tool == tool, true).clicked() {
                    self.set_tool(tool);
                    self.panel = Panel::None;
                }
            }
        });
        ui.add_space(8.0);
        let has = self.has_selection();
        ui.horizontal(|ui| {
            let sz = vec2(80.0, 76.0);
            if tile(ui, sz, Icon::Close, &t("Deselect"), false, has).clicked() && has {
                self.run("select.deselect", json!({}));
            }
            if tile(ui, sz, Icon::Refresh, &t("Invert"), false, has).clicked() && has {
                self.run("select.inverse", json!({}));
            }
            if tile(ui, sz, Icon::Bucket, &t("Fill"), false, has).clicked() && has {
                self.fill_selection();
            }
            if tile(ui, sz, Icon::Trash, &t("Clear"), false, has).clicked() && has {
                self.run("edit.clear", json!({}));
            }
        });
    }

    fn canvas_panel(&mut self, ui: &mut Ui) {
        ui.label(egui::RichText::new(t("Canvas")).size(19.0).color(TEXT).strong());
        ui.add_space(6.0);
        let sz = vec2(96.0, 76.0);
        let mut zoom_fit = false;
        ui.horizontal(|ui| {
            if tile(ui, sz, Icon::FlipH, &t("Flip canvas ↔"), false, true).clicked() {
                self.run("image.imageRotation.flipCanvasHorizontal", json!({}));
            }
            if tile(ui, sz, Icon::FlipV, &t("Flip canvas ↕"), false, true).clicked() {
                self.run("image.imageRotation.flipCanvasVertical", json!({}));
            }
            if tile(ui, sz, Icon::Canvas, &t("Fit to screen"), false, true).clicked() {
                zoom_fit = true;
            }
        });
        ui.horizontal(|ui| {
            if tile(ui, sz, Icon::FlipH, &t("Flip layer ↔"), false, true).clicked() {
                self.run("edit.transform.flipHorizontal", json!({}));
            }
            if tile(ui, sz, Icon::FlipV, &t("Flip layer ↕"), false, true).clicked() {
                self.run("edit.transform.flipVertical", json!({}));
            }
        });
        if zoom_fit {
            self.fitted = false;
            self.panel = Panel::None;
        }
    }

    fn blend_panel(&mut self, ui: &mut Ui, height: f32) {
        ui.label(egui::RichText::new(t("Blend mode")).size(19.0).color(TEXT).strong());
        let modes = ["Normal", "Multiply", "Screen", "Overlay", "Darken", "Lighten", "ColorDodge", "ColorBurn", "SoftLight", "HardLight", "Difference", "Exclusion", "Hue", "Saturation", "Color", "Luminosity", "LinearDodge"];
        egui::ScrollArea::vertical().max_height(height - 50.0).auto_shrink([false, false]).show(ui, |ui| {
            for m in modes {
                if ui::row(ui, m, "", false, PANEL_DARK, TEXT).clicked() {
                    if let Some(id) = self.active_layer() {
                        self.run("layer.setProps", json!({"layer": id, "blend": m}));
                    }
                    self.panel = Panel::Layers;
                }
            }
        });
    }

    fn export_dialog(&mut self, ctx: &Context) {
        let width = (ctx.content_rect().width() - 48.0).min(460.0);
        let mut close = false;
        let mut go = false;
        egui::Modal::new(Id::new("pc_export")).show(ctx, |ui| {
            ui.set_width(width);
            ui.label(egui::RichText::new(if self.export_share { t("Share") } else { t("Save Image") }).size(19.0).strong());
            ui.add_space(6.0);
            ui.horizontal_wrapped(|ui| {
                for f in Format::ALL {
                    if chip(ui, f.label(), self.export_format == f).clicked() {
                        self.export_format = f;
                    }
                }
            });
            if !crate::platform::get().persistent() {
                ui.label(egui::RichText::new(t("Browser preview: the file is downloaded.")).size(13.0).color(TEXT_DIM));
            }
            ui.add_space(8.0);
            ui.horizontal(|ui| {
                if ui::text_button(ui, &t("Cancel"), PANEL, TEXT, 100.0).clicked() {
                    close = true;
                }
                if ui::text_button(ui, &if self.export_share { t("Share") } else { t("Save") }, ACCENT, Color32::WHITE, 100.0).clicked() {
                    go = true;
                }
            });
        });
        if go {
            if let Some(doc) = self.doc() {
                match exporter::export(&self.store, &doc, &self.title, self.export_format, self.export_share) {
                    Ok(()) => {
                        if !self.export_share {
                            toast(t("Saved"));
                        }
                    }
                    Err(e) => toast(e),
                }
            }
            close = true;
        }
        if close {
            self.panel = Panel::None;
        }
    }

    fn help_dialog(&mut self, ctx: &Context) {
        let width = (ctx.content_rect().width() - 48.0).min(460.0);
        let mut close = false;
        egui::Modal::new(Id::new("pc_help")).show(ctx, |ui| {
            ui.set_width(width);
            ui.label(egui::RichText::new(t("How to Use")).size(19.0).strong());
            ui.add_space(6.0);
            for line in ["Draw with one finger.", "Pinch with two fingers to zoom, drag with two to move the canvas.", "The brush icon opens the tool menu; tap Brush there for the brush list.", "The square swatch opens the colour panel.", "The stack icon opens the layers.", "Your work is saved automatically."] {
                ui.label(egui::RichText::new(format!("• {}", t(line))).size(15.0));
            }
            ui.add_space(8.0);
            if ui::text_button(ui, &t("Close"), ACCENT, Color32::WHITE, 100.0).clicked() {
                close = true;
            }
        });
        if close {
            self.panel = Panel::None;
        }
    }

    fn text_and_rename_dialogs(&mut self, ctx: &Context) {
        if let Some((pos, mut text)) = self.text_dialog.take() {
            match ui::text_dialog(ctx, "pc_text", &t("Text"), &mut text, &t("Add")) {
                ui::DialogResult::Open => self.text_dialog = Some((pos, text)),
                ui::DialogResult::Ok => {
                    if !text.trim().is_empty() {
                        let hex = self.hex.clone();
                        let size = (self.size() * 1.5).max(10.0);
                        self.run("type.create", json!({"x": pos.x.round(), "y": pos.y.round(), "text": text, "size": size, "color": hex}));
                    }
                }
                ui::DialogResult::Cancel => {}
            }
        }
        if let Some((id, mut name)) = self.rename.take() {
            match ui::text_dialog(ctx, "pc_rename", &t("Layer name"), &mut name, &t("OK")) {
                ui::DialogResult::Open => self.rename = Some((id, name)),
                ui::DialogResult::Ok => {
                    self.run("layer.setProps", json!({"layer": id, "name": name}));
                }
                ui::DialogResult::Cancel => {}
            }
        }
    }
}

// ---- brushes, colour, layers, filters --------------------------------------------------------

impl Editor {
    fn brushes_panel(&mut self, ui: &mut Ui, height: f32) {
        let width = ui.available_width();
        ui.horizontal(|ui| {
            ui.label(egui::RichText::new(format!("{} ({})", t("Brushes"), BRUSHES.len())).size(20.0).color(TEXT).strong());
        });
        // Preview of the current brush.
        let (pr, _) = ui.allocate_exact_size(vec2(width, 64.0), Sense::hover());
        ui.painter().rect_filled(pr, 10.0, Color32::from_gray(0xee));
        brushes::preview(&ui.painter_at(pr), pr, &self.brush_def(), self.color32());
        ui.painter().text(pr.left_bottom() + vec2(10.0, -4.0), Align2::LEFT_BOTTOM, t(self.brush_def().name), FontId::proportional(15.0), Color32::BLACK);
        let list_h = (height - 64.0 - 36.0 - 2.0 * 44.0 - 40.0).max(120.0);
        ui.horizontal(|ui| {
            ui.spacing_mut().item_spacing.x = 4.0;
            // categories
            let (sr, _) = ui.allocate_exact_size(vec2(84.0, list_h), Sense::hover());
            ui.painter().rect_filled(sr, 8.0, SIDEBAR);
            let mut y = sr.top() + 4.0;
            for (i, cat) in Category::LIST.iter().enumerate() {
                let r = Rect::from_min_size(pos2(sr.left(), y), vec2(sr.width(), 52.0));
                let resp = ui.interact(r, Id::new(("pc_cat", i)), Sense::click());
                let on = self.brush_cat == i;
                let col = if on { ACCENT } else { Color32::from_gray(0xe0) };
                ui.painter().text(r.center(), Align2::CENTER_CENTER, t(cat.label()), FontId::proportional(13.5), col);
                if resp.clicked() {
                    self.brush_cat = i;
                }
                y += 52.0;
            }
            // list
            let lw = ui.available_width();
            let cat = Category::LIST[self.brush_cat.min(Category::LIST.len() - 1)];
            ui.allocate_ui(vec2(lw, list_h), |ui| {
                ui.set_width(lw);
                egui::ScrollArea::vertical().id_salt("pc_brush_list").max_height(list_h).auto_shrink([false, false]).show(ui, |ui| {
                    ui.spacing_mut().item_spacing.y = 0.0;
                    for i in 0..BRUSHES.len() {
                        let def = BRUSHES[i];
                        if cat != Category::All && def.category != cat {
                            continue;
                        }
                        let (rr, resp) = ui.allocate_exact_size(vec2(lw - 6.0, 70.0), Sense::click());
                        let selected = self.brush == i && self.tool == Tool::Brush;
                        ui.painter().rect_filled(rr, 0.0, if selected { Color32::from_rgb(0xd2, 0xe3, 0xfa) } else { Color32::from_gray(0xf7) });
                        let pv = Rect::from_min_size(rr.min + vec2(4.0, 2.0), vec2(rr.width() - 64.0, 44.0));
                        brushes::preview(&ui.painter_at(pv), pv, &def, Color32::BLACK);
                        ui.painter().text(rr.left_bottom() + vec2(8.0, -6.0), Align2::LEFT_BOTTOM, t(def.name), FontId::proportional(15.0), Color32::from_gray(0x20));
                        ui.painter().text(pos2(rr.right() - 58.0, rr.top() + 6.0), Align2::RIGHT_TOP, format!("{:.1}", def.size), FontId::proportional(14.0), Color32::from_gray(0x40));
                        let gear = Rect::from_center_size(pos2(rr.right() - 26.0, rr.center().y + 4.0), vec2(40.0, 40.0));
                        let gr = ui.interact(gear, Id::new(("pc_brush_cfg", i)), Sense::click());
                        ui.painter().circle_stroke(gear.center(), 18.0, Stroke::new(1.5, Color32::from_gray(0x70)));
                        icons::paint(ui.painter(), gear.shrink(11.0), if self.brush_open == Some(i) { Icon::Down } else { Icon::Right }, Color32::from_gray(0x50));
                        if gr.clicked() {
                            self.brush_open = if self.brush_open == Some(i) { None } else { Some(i) };
                        } else if resp.clicked() {
                            self.select_brush(i);
                        }
                        if self.brush_open == Some(i) {
                            egui::Frame::new().fill(Color32::from_gray(0xe4)).inner_margin(Margin::same(6)).show(ui, |ui| {
                                ui.set_width(lw - 18.0);
                                let t0 = self.tweaks[i];
                                let mut v = t0;
                                for (k, (name, lo, hi)) in [("Hardness", 0.0f32, 1.0f32), ("Flow", 0.05, 1.0), ("Spacing", 0.02, 1.0)].iter().enumerate() {
                                    ui.horizontal(|ui| {
                                        ui.add_sized([70.0, 30.0], egui::Label::new(egui::RichText::new(t(name)).color(Color32::BLACK).size(13.5)));
                                        ui::slider(ui, ui.available_width(), &mut v[k], *lo, *hi, false);
                                    });
                                }
                                if v != t0 {
                                    self.tweaks[i] = v;
                                }
                            });
                        }
                    }
                });
            });
        });
        ui.add_space(2.0);
        let slot = self.size_slot();
        let mut size = self.sizes[slot];
        if ui::stepper_slider(ui, "pc_bsize", &format!("{size:.1}"), &mut size, 1.0, 500.0, true, 1.0, 1) {
            self.sizes[slot] = size;
        }
        let mut pct = self.opacity * 100.0;
        if ui::stepper_slider(ui, "pc_bopa", &format!("{pct:.0}"), &mut pct, 1.0, 100.0, false, 1.0, 0) {
            self.opacity = pct / 100.0;
        }
    }

    fn color_panel(&mut self, ui: &mut Ui, height: f32) {
        ui.horizontal(|ui| {
            ui.label(egui::RichText::new(t("Color")).size(20.0).color(TEXT).strong());
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                let (r, _) = ui.allocate_exact_size(vec2(120.0, 34.0), Sense::hover());
                let (a, b) = r.split_left_right_at_fraction(0.5);
                ui.painter().rect_filled(a, 6.0, self.color32());
                let prev = self.recent.get(1).copied().unwrap_or([0, 0, 0]);
                ui.painter().rect_filled(b, 6.0, Color32::from_rgb(prev[0], prev[1], prev[2]));
                ui::stroke_rect(ui.painter(), r, 6.0, Color32::BLACK, 2.0);
            });
        });
        let body_h = height - 40.0 - 52.0;
        egui::ScrollArea::vertical().id_salt("pc_color_body").max_height(body_h).auto_shrink([false, false]).show(ui, |ui| match self.color_tab {
            ColorTab::Hsb => {
                let side = ui.available_width().min(body_h - 150.0).clamp(150.0, 300.0);
                ui.vertical_centered(|ui| self.color_wheel(ui, side));
                let mut h = self.hsv[0] * 360.0;
                let mut s = self.hsv[1] * 100.0;
                let mut v = self.hsv[2] * 100.0;
                let mut ch = false;
                ch |= ui::stepper_slider(ui, "pc_h", &format!("{h:.0}°"), &mut h, 0.0, 360.0, false, 1.0, 0);
                ch |= ui::stepper_slider(ui, "pc_s", &format!("{s:.0}%"), &mut s, 0.0, 100.0, false, 1.0, 0);
                ch |= ui::stepper_slider(ui, "pc_v", &format!("{v:.0}%"), &mut v, 0.0, 100.0, false, 1.0, 0);
                if ch {
                    self.hsv = [h / 360.0, s / 100.0, v / 100.0];
                    self.sync_hex();
                }
                self.hex_field(ui);
            }
            ColorTab::Rgb => {
                let mut c = self.rgb8();
                let mut ch = false;
                for (i, name) in ["R", "G", "B"].iter().enumerate() {
                    let mut v = c[i] as f32;
                    if ui::stepper_slider(ui, name, &format!("{name} {v:.0}"), &mut v, 0.0, 255.0, false, 1.0, 0) {
                        c[i] = v as u8;
                        ch = true;
                    }
                }
                if ch {
                    self.set_rgb(c);
                }
                self.hex_field(ui);
            }
            ColorTab::Palette => {
                let mut pick = None;
                ui.label(egui::RichText::new(t("Palette")).color(TEXT_DIM));
                ui.horizontal_wrapped(|ui| {
                    for c in PALETTE {
                        if swatch(ui, c) {
                            pick = Some(c);
                        }
                    }
                });
                if !self.recent.is_empty() {
                    ui.label(egui::RichText::new(t("Recent")).color(TEXT_DIM));
                    ui.horizontal_wrapped(|ui| {
                        for c in self.recent.clone() {
                            if swatch(ui, c) {
                                pick = Some(c);
                            }
                        }
                    });
                }
                if let Some(c) = pick {
                    self.set_rgb(c);
                }
            }
        });
        // tabs
        let (rect, _) = ui.allocate_exact_size(vec2(ui.available_width(), 48.0), Sense::hover());
        ui.painter().rect_filled(rect, 8.0, Color32::BLACK);
        let third = rect.width() / 3.0;
        for (i, (tab, label)) in [(ColorTab::Palette, "Palette"), (ColorTab::Rgb, "RGB"), (ColorTab::Hsb, "HSB")].iter().enumerate() {
            let r = Rect::from_min_size(pos2(rect.left() + third * i as f32, rect.top()), vec2(third, rect.height()));
            let resp = ui.interact(r, Id::new(("pc_ctab", i)), Sense::click());
            ui.painter().text(r.center(), Align2::CENTER_CENTER, t(label), FontId::proportional(15.0), if self.color_tab == *tab { ACCENT } else { Color32::WHITE });
            if resp.clicked() {
                self.color_tab = *tab;
            }
        }
    }

    fn hex_field(&mut self, ui: &mut Ui) {
        ui.horizontal(|ui| {
            ui.label(egui::RichText::new("#").color(TEXT).size(18.0));
            let mut text = self.hex.trim_start_matches('#').to_string();
            let r = ui.add(egui::TextEdit::singleline(&mut text).desired_width(120.0).font(FontId::monospace(18.0)));
            if r.changed() {
                let clean: String = text.chars().filter(|c| c.is_ascii_hexdigit()).take(6).collect();
                if clean.len() == 6 {
                    if let Ok(v) = u32::from_str_radix(&clean, 16) {
                        self.set_rgb([(v >> 16) as u8, (v >> 8) as u8, v as u8]);
                    }
                } else {
                    self.hex = format!("#{clean}");
                }
            }
        });
    }

    fn color_wheel(&mut self, ui: &mut Ui, side: f32) {
        let (rect, resp) = ui.allocate_exact_size(vec2(side, side), Sense::click_and_drag());
        let c = rect.center();
        let ro = side * 0.5;
        let ri = ro * 0.76;
        let q = ri * 0.68;
        let to_c = |rgb: [f32; 3]| Color32::from_rgb((rgb[0] * 255.0) as u8, (rgb[1] * 255.0) as u8, (rgb[2] * 255.0) as u8);
        let mut mesh = egui::epaint::Mesh::default();
        let n = 96;
        for i in 0..n {
            let (a0, a1) = (i as f32 / n as f32, (i + 1) as f32 / n as f32);
            let (t0, t1) = (a0 * std::f32::consts::TAU, a1 * std::f32::consts::TAU);
            let (c0, c1) = (to_c(hsv_to_rgb(a0, 1.0, 1.0)), to_c(hsv_to_rgb(a1, 1.0, 1.0)));
            let idx = mesh.vertices.len() as u32;
            mesh.colored_vertex(c + vec2(t0.cos(), t0.sin()) * ro, c0);
            mesh.colored_vertex(c + vec2(t0.cos(), t0.sin()) * ri, c0);
            mesh.colored_vertex(c + vec2(t1.cos(), t1.sin()) * ro, c1);
            mesh.colored_vertex(c + vec2(t1.cos(), t1.sin()) * ri, c1);
            mesh.add_triangle(idx, idx + 1, idx + 2);
            mesh.add_triangle(idx + 1, idx + 3, idx + 2);
        }
        let g = 14u32;
        let base = mesh.vertices.len() as u32;
        for yi in 0..=g {
            for xi in 0..=g {
                let (s, v) = (xi as f32 / g as f32, 1.0 - yi as f32 / g as f32);
                mesh.colored_vertex(c + vec2((s - 0.5) * 2.0 * q, (0.5 - v) * 2.0 * q), to_c(hsv_to_rgb(self.hsv[0], s, v)));
            }
        }
        for yi in 0..g {
            for xi in 0..g {
                let i0 = base + yi * (g + 1) + xi;
                mesh.add_triangle(i0, i0 + 1, i0 + g + 1);
                mesh.add_triangle(i0 + 1, i0 + g + 2, i0 + g + 1);
            }
        }
        ui.painter().add(Shape::mesh(mesh));
        let ang = self.hsv[0] * std::f32::consts::TAU;
        let hr = (ro + ri) * 0.5;
        let hp = c + vec2(ang.cos(), ang.sin()) * hr;
        ui.painter().circle_stroke(hp, 11.0, Stroke::new(3.0, Color32::WHITE));
        ui.painter().circle_stroke(hp, 13.0, Stroke::new(1.0, Color32::BLACK));
        let sp = c + vec2((self.hsv[1] - 0.5) * 2.0 * q, (0.5 - self.hsv[2]) * 2.0 * q);
        ui.painter().circle_stroke(sp, 10.0, Stroke::new(3.0, Color32::WHITE));
        ui.painter().circle_stroke(sp, 12.0, Stroke::new(1.0, Color32::BLACK));
        if resp.is_pointer_button_down_on() || resp.dragged() {
            if let Some(p) = resp.interact_pointer_pos() {
                let d = p - c;
                if self.wheel.is_none() {
                    self.wheel = Some(if d.length() > ri * 0.97 { WheelPart::Ring } else { WheelPart::Square });
                }
                match self.wheel {
                    Some(WheelPart::Ring) => self.hsv[0] = (d.y.atan2(d.x) / std::f32::consts::TAU).rem_euclid(1.0),
                    Some(WheelPart::Square) => {
                        self.hsv[1] = (d.x / (2.0 * q) + 0.5).clamp(0.0, 1.0);
                        self.hsv[2] = (0.5 - d.y / (2.0 * q)).clamp(0.0, 1.0);
                    }
                    None => {}
                }
                self.sync_hex();
            }
        } else {
            self.wheel = None;
        }
    }

    fn layers_panel(&mut self, ui: &mut Ui, height: f32) {
        let Some(doc) = self.doc() else { return };
        let active = self.active_layer();
        ui.horizontal(|ui| {
            ui.label(egui::RichText::new(format!("{} ({})", t("Layers"), doc.layers.len())).size(20.0).color(TEXT).strong());
        });
        ui.horizontal(|ui| {
            ui.spacing_mut().item_spacing.x = 2.0;
            let sz = vec2(((ui.available_width() - 10.0) / 6.0).min(70.0), 58.0);
            if tile(ui, sz, Icon::Plus, &t("Add"), false, true).clicked() {
                self.run("layer.new.layer", json!({}));
            }
            if tile(ui, sz, Icon::Copy, &t("Duplicate"), false, active.is_some()).clicked() {
                self.run("layer.duplicate", json!({}));
            }
            if tile(ui, sz, Icon::Merge, &t("Merge down"), false, doc.layers.len() > 1).clicked() {
                self.run("layer.mergeDown", json!({}));
            }
            if tile(ui, sz, Icon::Image, &t("Import"), false, true).clicked() {
                self.placing = true;
                crate::platform::get().pick_picture();
            }
            if tile(ui, sz, Icon::FlipH, &t("Flip"), false, active.is_some()).clicked() {
                self.run("edit.transform.flipHorizontal", json!({}));
            }
            if tile(ui, sz, Icon::Trash, &t("Delete"), false, doc.layers.len() > 1).clicked() && doc.layers.len() > 1 {
                self.run("layer.delete", json!({}));
            }
        });
        let list_h = (height - 40.0 - 62.0 - 130.0 - 26.0).max(90.0);
        egui::ScrollArea::vertical().id_salt("pc_layer_list").max_height(list_h).auto_shrink([false, false]).show(ui, |ui| {
            ui.spacing_mut().item_spacing.y = 3.0;
            let n = doc.layers.len();
            for (pos, layer) in doc.layers.iter().enumerate().rev() {
                let id = layer.id.0;
                let (rr, resp) = ui.allocate_exact_size(vec2(ui.available_width(), 56.0), Sense::click());
                let selected = Some(id) == active;
                ui.painter().rect_filled(rr, 8.0, if selected { Color32::from_rgb(0xc9, 0xd9, 0xf4) } else { Color32::from_gray(0xeb) });
                let eye = Rect::from_center_size(pos2(rr.left() + 26.0, rr.center().y), vec2(44.0, 44.0));
                let er = ui.interact(eye, Id::new(("pc_eye", id)), Sense::click());
                icons::paint(ui.painter(), eye.shrink(10.0), if layer.visible { Icon::Eye } else { Icon::EyeOff }, Color32::from_gray(0x30));
                let th = Rect::from_center_size(pos2(rr.left() + 76.0, rr.center().y), vec2(40.0, 40.0));
                ui::checker(&ui.painter_at(th), th, 8.0);
                ui::stroke_rect(ui.painter(), th, 2.0, Color32::from_gray(0x60), 1.0);
                ui.painter().text(pos2(th.right() + 10.0, rr.center().y - 9.0), Align2::LEFT_CENTER, &layer.name, FontId::proportional(16.0), Color32::BLACK);
                ui.painter().text(pos2(th.right() + 10.0, rr.center().y + 11.0), Align2::LEFT_CENTER, format!("{:.0}%  {:?}", layer.opacity * 100.0, layer.blend), FontId::proportional(12.5), Color32::from_gray(0x50));
                let up = Rect::from_center_size(pos2(rr.right() - 70.0, rr.center().y), vec2(40.0, 44.0));
                let dn = Rect::from_center_size(pos2(rr.right() - 26.0, rr.center().y), vec2(40.0, 44.0));
                let ur = ui.interact(up, Id::new(("pc_up", id)), Sense::click());
                let dr = ui.interact(dn, Id::new(("pc_dn", id)), Sense::click());
                icons::paint(ui.painter(), up.shrink2(vec2(10.0, 12.0)), Icon::Up, Color32::from_gray(if pos + 1 < n { 0x30 } else { 0xb0 }));
                icons::paint(ui.painter(), dn.shrink2(vec2(10.0, 12.0)), Icon::Down, Color32::from_gray(if pos > 0 { 0x30 } else { 0xb0 }));
                if er.clicked() {
                    self.run("layer.setProps", json!({"layer": id, "visible": !layer.visible}));
                } else if ur.clicked() && pos + 1 < n {
                    let target = doc.layers[pos + 1].id.0;
                    self.run("layer.moveTo", json!({"layer": id, "target": target, "position": "above"}));
                } else if dr.clicked() && pos > 0 {
                    let target = doc.layers[pos - 1].id.0;
                    self.run("layer.moveTo", json!({"layer": id, "target": target, "position": "below"}));
                } else if resp.double_clicked() {
                    self.rename = Some((id, layer.name.clone()));
                } else if resp.clicked() {
                    self.run("layer.select", json!({"layer": id}));
                }
            }
        });
        ui.add_space(2.0);
        // properties of the active layer
        if let Some(layer) = doc.layers.iter().find(|l| Some(l.id.0) == active) {
            ui.horizontal(|ui| {
                if ui::text_button(ui, &format!("{:?}  ⌃", layer.blend), Color32::WHITE, Color32::BLACK, 130.0).clicked() {
                    self.panel = Panel::Blend;
                }
                let clip = layer.clipped;
                if chip(ui, &t("Clip"), clip).clicked() {
                    self.run("layer.setProps", json!({"layer": layer.id.0, "clipped": !clip}));
                }
                let locked = layer.locks.transparency;
                if chip(ui, &t("Lock α"), locked).clicked() {
                    self.run("layer.setProps", json!({"layer": layer.id.0, "locks": {"transparency": !locked}}));
                }
            });
            let mut op = layer.opacity * 100.0;
            ui.horizontal(|ui| {
                ui.add_sized([56.0, 34.0], egui::Label::new(egui::RichText::new(format!("{op:.0}%")).color(TEXT)));
                let (_, released) = ui::slider_ex(ui, ui.available_width(), &mut op, 0.0, 100.0, false);
                if released {
                    self.run("layer.setProps", json!({"layer": layer.id.0, "opacity": op / 100.0}));
                }
            });
        }
        ui.horizontal(|ui| {
            ui.label(egui::RichText::new(t("Background")).color(TEXT_DIM).size(14.0));
            for (i, col) in [Color32::WHITE, Color32::from_gray(0xd0), Color32::from_gray(0x38), Color32::TRANSPARENT].iter().enumerate() {
                let (r, resp) = ui.allocate_exact_size(vec2(34.0, 28.0), Sense::click());
                if i == 1 {
                    ui::checker(&ui.painter_at(r), r, 7.0);
                } else {
                    ui.painter().rect_filled(r, 3.0, *col);
                    if i == 3 {
                        ui.painter().line_segment([r.left_bottom(), r.right_top()], Stroke::new(1.5, Color32::BLACK));
                    }
                }
                ui::stroke_rect(ui.painter(), r, 3.0, if self.canvas_bg == i { ACCENT } else { Color32::BLACK }, if self.canvas_bg == i { 3.0 } else { 1.0 });
                if resp.clicked() {
                    self.canvas_bg = i;
                }
            }
        });
    }

    fn filters_panel(&mut self, ui: &mut Ui, height: f32) {
        ui.label(egui::RichText::new(t("Filter")).size(20.0).color(TEXT).strong());
        if let Some(kind) = self.filter {
            let (title, a_label, a_range) = match kind {
                FilterKind::Gaussian => ("Gaussian Blur", "Radius", (0.5, 60.0)),
                FilterKind::Noise => ("Add Noise", "Amount", (1.0, 100.0)),
                FilterKind::BrightnessContrast => ("Brightness / Contrast", "Brightness", (-100.0, 100.0)),
                _ => ("", "", (0.0, 1.0)),
            };
            ui.label(egui::RichText::new(t(title)).size(17.0).color(TEXT));
            if matches!(kind, FilterKind::Gaussian | FilterKind::Noise | FilterKind::BrightnessContrast) {
                ui.horizontal(|ui| {
                    ui.add_sized([100.0, 34.0], egui::Label::new(egui::RichText::new(format!("{} {:.1}", t(a_label), self.filter_a)).color(TEXT)));
                    let w = ui.available_width();
                    let mut a = self.filter_a;
                    if ui::slider(ui, w, &mut a, a_range.0, a_range.1, false) {
                        self.filter_a = a;
                    }
                });
            }
            if kind == FilterKind::BrightnessContrast {
                ui.horizontal(|ui| {
                    ui.add_sized([100.0, 34.0], egui::Label::new(egui::RichText::new(format!("{} {:.0}", t("Contrast"), self.filter_b)).color(TEXT)));
                    let w = ui.available_width();
                    let mut b = self.filter_b;
                    if ui::slider(ui, w, &mut b, -100.0, 100.0, false) {
                        self.filter_b = b;
                    }
                });
            }
            ui.add_space(8.0);
            ui.horizontal(|ui| {
                if ui::text_button(ui, &t("Cancel"), PANEL_DEEP, TEXT, 110.0).clicked() {
                    self.filter = None;
                }
                if ui::text_button(ui, &t("Apply"), ACCENT, Color32::WHITE, 110.0).clicked() {
                    self.apply_filter(kind);
                    self.filter = None;
                }
            });
            return;
        }
        egui::ScrollArea::vertical().id_salt("pc_filters").max_height(height - 50.0).auto_shrink([false, false]).show(ui, |ui| {
            for (kind, label) in [(FilterKind::Gaussian, "Gaussian Blur"), (FilterKind::Sharpen, "Sharpen"), (FilterKind::Noise, "Add Noise"), (FilterKind::BrightnessContrast, "Brightness / Contrast"), (FilterKind::Invert, "Invert Colors"), (FilterKind::Grayscale, "Black & White")] {
                if ui::row(ui, &t(label), "", false, PANEL_DARK, TEXT).clicked() {
                    match kind {
                        FilterKind::Gaussian => {
                            self.filter_a = 5.0;
                            self.filter = Some(kind);
                        }
                        FilterKind::Noise => {
                            self.filter_a = 12.0;
                            self.filter = Some(kind);
                        }
                        FilterKind::BrightnessContrast => {
                            self.filter_a = 0.0;
                            self.filter_b = 0.0;
                            self.filter = Some(kind);
                        }
                        _ => self.apply_filter(kind),
                    }
                }
            }
        });
    }

    fn apply_filter(&mut self, kind: FilterKind) {
        let (a, b) = (self.filter_a, self.filter_b);
        match kind {
            FilterKind::Gaussian => self.run("filter.blur.gaussianBlur", json!({"radius": a})),
            FilterKind::Sharpen => self.run("filter.sharpen.sharpen", json!({})),
            FilterKind::Noise => self.run("filter.noise.addNoise", json!({"amount": a})),
            FilterKind::BrightnessContrast => self.run("image.adjustments.brightnessContrast", json!({"brightness": a, "contrast": b})),
            FilterKind::Invert => self.run("image.adjustments.invert", json!({})),
            FilterKind::Grayscale => self.run("image.adjustments.desaturate", json!({})),
        };
    }
}

fn swatch(ui: &mut Ui, c: [u8; 3]) -> bool {
    let (r, resp) = ui.allocate_exact_size(vec2(40.0, 40.0), Sense::click());
    ui.painter().rect_filled(r, 8.0, Color32::from_rgb(c[0], c[1], c[2]));
    ui::stroke_rect(ui.painter(), r, 8.0, Color32::from_black_alpha(120), 1.0);
    resp.clicked()
}
