//! My Gallery: the grid of artworks and folders, the New canvas sheet, work information, the
//! recently-deleted bin, moving, duplicating, exporting. Modelled on ibisPaint's gallery.

use std::collections::{HashMap, HashSet};

use egui::{Align2, Color32, ColorImage, Context, FontId, Id, Margin, Rect, Sense, Stroke, TextureHandle, TextureOptions, Ui, pos2, vec2};

use crate::exporter::{self, Format};
use crate::i18n::{self, t};
use crate::icons::{self, Icon};
use crate::platform;
use crate::store::{self, Entry, Store};
use crate::ui::{self, ACCENT, BAR, DANGER, GALLERY_BG, PANEL, PANEL_DARK, SHEET, TEXT, TEXT_DIM, toast};

/// What the gallery asks the app to do.
pub enum Action {
    Open(String),
    New { width: u32, height: u32, dpi: f32, parent: String },
    Import { parent: String },
}

enum Prompt {
    NewFolder(String),
    Rename(String, String),
}

enum Confirm {
    Purge(Vec<String>),
    Trash(Vec<String>),
    EmptyBin,
}

struct Info {
    id: String,
    title: String,
    artist: String,
    description: String,
}

struct ExportDlg {
    ids: Vec<String>,
    format: Format,
    share: bool,
}

pub struct Gallery {
    store: Store,
    entries: Vec<Entry>,
    loaded: bool,
    pub folder: String,
    selected: Option<String>,
    multi: bool,
    picks: HashSet<String>,
    thumbs: HashMap<String, (i64, Option<TextureHandle>)>,
    menu: bool,
    edit_sheet: bool,
    new_sheet: bool,
    custom: (String, String),
    prompt: Option<Prompt>,
    confirm: Option<Confirm>,
    info: Option<Info>,
    export: Option<ExportDlg>,
    move_ids: Option<Vec<String>>,
    bin: bool,
    path_dialog: bool,
}

const PRESETS: &[(&str, &str, u32, u32, f32)] = &[
    ("HD (Quality)", "", 1440, 3208, 72.0),
    ("1 : 1 (2K)", "", 2048, 2048, 72.0),
    ("3 : 4 (2K)", "", 1536, 2048, 72.0),
    ("9 : 16 (2K)", "", 1080, 1920, 72.0),
    ("Header X", "", 1500, 500, 72.0),
    ("Chat Trace", "", 370, 320, 72.0),
    ("Vertical", "", 690, 4096, 72.0),
];

const PRINT_PRESETS: &[(&str, u32, u32, f32)] = &[("Postcard (JP)", 1181, 1748, 300.0), ("A4", 1240, 1754, 150.0), ("A5", 874, 1240, 150.0), ("B4", 1476, 2085, 150.0), ("B5", 1039, 1476, 150.0)];

/// The largest canvas (pixels) the engine's float compositor handles comfortably on a phone.
pub const MAX_PIXELS: u64 = 4_800_000;
pub const MAX_SIDE: u32 = 4096;

impl Gallery {
    pub fn new(store: Store) -> Gallery {
        Gallery {
            store,
            entries: Vec::new(),
            loaded: false,
            folder: String::new(),
            selected: None,
            multi: false,
            picks: HashSet::new(),
            thumbs: HashMap::new(),
            menu: false,
            edit_sheet: false,
            new_sheet: false,
            custom: ("1316".into(), "836".into()),
            prompt: None,
            confirm: None,
            info: None,
            export: None,
            move_ids: None,
            bin: false,
            path_dialog: false,
        }
    }

    pub fn refresh(&mut self) {
        self.loaded = false;
    }

    fn reload(&mut self) {
        self.entries = self.store.scan();
        self.loaded = true;
        let live: HashSet<&String> = self.entries.iter().map(|e| &e.id).collect();
        self.thumbs.retain(|k, _| live.contains(k));
        if let Some(s) = &self.selected {
            if !self.entries.iter().any(|e| &e.id == s && e.meta.deleted == 0) {
                self.selected = None;
            }
        }
    }

    fn visible(&self) -> Vec<Entry> {
        let mut v: Vec<Entry> = self.entries.iter().filter(|e| e.meta.deleted == 0 && e.meta.parent == self.folder).cloned().collect();
        v.sort_by(|a, b| b.meta.folder.cmp(&a.meta.folder).then_with(|| b.meta.updated.cmp(&a.meta.updated)));
        v
    }

    fn binned(&self) -> Vec<Entry> {
        let mut v: Vec<Entry> = self.entries.iter().filter(|e| e.meta.deleted != 0).cloned().collect();
        v.sort_by(|a, b| b.meta.deleted.cmp(&a.meta.deleted));
        v
    }

    fn title_of(&self, id: &str) -> String {
        self.entries.iter().find(|e| e.id == id).map(|e| e.meta.title.clone()).unwrap_or_default()
    }

    fn thumb(&mut self, ctx: &Context, e: &Entry) -> Option<TextureHandle> {
        let stale = self.thumbs.get(&e.id).is_none_or(|(stamp, _)| *stamp != e.meta.updated);
        if stale {
            let tex = self.store.thumb(&e.id).map(|th| {
                let img = ColorImage::from_rgba_unmultiplied([th.width as usize, th.height as usize], &th.rgba);
                ctx.load_texture(format!("thumb-{}", e.id), img, TextureOptions::LINEAR)
            });
            self.thumbs.insert(e.id.clone(), (e.meta.updated, tex));
        }
        self.thumbs.get(&e.id).and_then(|(_, t)| t.clone())
    }

    fn ids_for_action(&self) -> Vec<String> {
        if self.multi && !self.picks.is_empty() {
            self.picks.iter().cloned().collect()
        } else {
            self.selected.iter().cloned().collect()
        }
    }

    pub fn back(&mut self) -> bool {
        if self.new_sheet || self.menu || self.edit_sheet {
            self.new_sheet = false;
            self.menu = false;
            self.edit_sheet = false;
            true
        } else if self.bin {
            self.bin = false;
            true
        } else if self.multi {
            self.multi = false;
            self.picks.clear();
            true
        } else if !self.folder.is_empty() {
            self.go_up();
            true
        } else {
            false
        }
    }

    fn go_up(&mut self) {
        let parent = self.entries.iter().find(|e| e.id == self.folder).map(|e| e.meta.parent.clone()).unwrap_or_default();
        self.folder = parent;
        self.selected = None;
    }

    pub fn count_here(&self) -> usize {
        self.visible().iter().filter(|e| !e.meta.folder).count()
    }

    pub fn ui(&mut self, ui: &mut Ui) -> Option<Action> {
        if !self.loaded {
            self.reload();
        }
        let ctx = ui.ctx().clone();
        let mut action = None;
        self.top_bar(ui);
        if self.bin {
            self.bin_view(ui, &ctx);
            self.dialogs(&ctx, &mut action);
            return action;
        }
        self.bottom_bar(ui, &mut action);
        self.info_strip(ui);
        egui::CentralPanel::default().frame(egui::Frame::NONE.fill(GALLERY_BG)).show(ui, |ui| {
            self.grid(ui, &ctx, &mut action);
        });
        self.popups(&ctx, &mut action);
        self.dialogs(&ctx, &mut action);
        action
    }

    // ---- bars ------------------------------------------------------------------------------

    fn top_bar(&mut self, ui: &mut Ui) {
        let link = Color32::from_rgb(0x6f, 0xa8, 0xff);
        egui::Panel::top("pc_g_top").exact_size(54.0).frame(egui::Frame::NONE.fill(BAR).inner_margin(Margin::symmetric(12, 0))).show(ui, |ui| {
            let rect = ui.max_rect();
            let title = if self.bin {
                format!("{} ({})", t("Recently Deleted"), self.binned().len())
            } else if self.folder.is_empty() {
                format!("{} ({})", t("My Gallery"), self.entries.iter().filter(|e| e.meta.deleted == 0 && !e.meta.folder).count())
            } else {
                format!("{} ({})", self.title_of(&self.folder), self.count_here())
            };
            ui.painter().text(rect.center(), Align2::CENTER_CENTER, title, FontId::proportional(19.0), Color32::WHITE);
            let back_visible = self.bin || !self.folder.is_empty() || self.multi;
            if back_visible {
                let r = Rect::from_min_size(rect.left_top(), vec2(110.0, rect.height()));
                let resp = ui.interact(r, Id::new("pc_g_back"), Sense::click());
                icons::paint(ui.painter(), Rect::from_center_size(pos2(r.left() + 12.0, r.center().y), vec2(26.0, 26.0)), Icon::Left, link);
                ui.painter().text(pos2(r.left() + 30.0, r.center().y), Align2::LEFT_CENTER, t("Back"), FontId::proportional(18.0), link);
                if resp.clicked() {
                    self.back();
                }
            }
            let (label, on) = if self.bin { (t("Empty"), true) } else if self.multi { (t("Done"), true) } else { (t("Select"), true) };
            let r = Rect::from_min_size(pos2(rect.right() - 110.0, rect.top()), vec2(110.0, rect.height()));
            let resp = ui.interact(r, Id::new("pc_g_sel"), Sense::click());
            ui.painter().text(pos2(r.right() - 4.0, r.center().y), Align2::RIGHT_CENTER, label, FontId::proportional(18.0), if on { link } else { TEXT_DIM });
            if resp.clicked() {
                if self.bin {
                    self.confirm = Some(Confirm::EmptyBin);
                } else {
                    self.multi = !self.multi;
                    self.picks.clear();
                }
            }
        });
    }

    fn bottom_bar(&mut self, ui: &mut Ui, action: &mut Option<Action>) {
        let link = Color32::from_rgb(0x6f, 0xa8, 0xff);
        egui::Panel::bottom("pc_g_bottom").exact_size(60.0).frame(egui::Frame::NONE.fill(BAR)).show(ui, |ui| {
            let rect = ui.max_rect();
            let ids = self.ids_for_action();
            let has = !ids.is_empty();
            let items: Vec<(Icon, bool)> = if self.multi {
                vec![(Icon::Folder, has), (Icon::Copy, has), (Icon::Down, has), (Icon::Share, has), (Icon::Trash, has)]
            } else {
                vec![(Icon::Plus, true), (Icon::Info, has), (Icon::Play, has), (Icon::Share, has), (Icon::More, true)]
            };
            let n = items.len() + usize::from(!self.multi);
            let w = rect.width() / n as f32;
            for (i, (icon, enabled)) in items.iter().enumerate() {
                let r = Rect::from_min_size(pos2(rect.left() + w * i as f32, rect.top()), vec2(w, rect.height()));
                let resp = ui.interact(r, Id::new(("pc_g_btn", i)), Sense::click());
                let col = if *enabled { link } else { Color32::from_gray(0x55) };
                icons::paint(ui.painter(), Rect::from_center_size(r.center(), vec2(34.0, 34.0)), *icon, col);
                if resp.clicked() && *enabled {
                    self.bottom_action(i, &ids, action);
                }
            }
            if !self.multi {
                let r = Rect::from_min_size(pos2(rect.left() + w * items.len() as f32, rect.top()), vec2(w, rect.height()));
                let resp = ui.interact(r, Id::new("pc_g_edit"), Sense::click());
                ui.painter().text(r.center(), Align2::CENTER_CENTER, t("Edit"), FontId::proportional(18.0), if has { link } else { Color32::from_gray(0x55) });
                if resp.clicked() && has {
                    self.edit_sheet = true;
                }
            }
        });
    }

    fn bottom_action(&mut self, i: usize, ids: &[String], action: &mut Option<Action>) {
        if self.multi {
            match i {
                0 => self.move_ids = Some(ids.to_vec()),
                1 => {
                    for id in ids {
                        let suffix = format!(" {}", t("(copy)"));
                        if let Err(e) = self.store.duplicate(id, &suffix) {
                            toast(e);
                        }
                    }
                    self.loaded = false;
                    self.picks.clear();
                }
                2 => self.export = Some(ExportDlg { ids: ids.to_vec(), format: Format::Png, share: false }),
                3 => self.export = Some(ExportDlg { ids: ids.to_vec(), format: Format::Png, share: true }),
                _ => self.confirm = Some(Confirm::Trash(ids.to_vec())),
            }
            return;
        }
        match i {
            0 => self.new_sheet = true,
            1 => {
                if let Some(id) = ids.first() {
                    if let Some(e) = self.entries.iter().find(|e| &e.id == id) {
                        self.info = Some(Info { id: id.clone(), title: e.meta.title.clone(), artist: e.meta.artist.clone(), description: e.meta.description.clone() });
                    }
                }
            }
            2 => {
                if let Some(id) = ids.first() {
                    self.open_item(id, action);
                }
            }
            3 => self.export = Some(ExportDlg { ids: ids.to_vec(), format: Format::Png, share: true }),
            _ => self.menu = true,
        }
    }

    fn open_item(&mut self, id: &str, action: &mut Option<Action>) {
        let Some(e) = self.entries.iter().find(|e| e.id == id) else { return };
        if e.meta.folder {
            self.folder = id.to_string();
            self.selected = None;
        } else {
            *action = Some(Action::Open(id.to_string()));
        }
    }

    fn info_strip(&mut self, ui: &mut Ui) {
        let Some(id) = self.selected.clone() else { return };
        let Some(e) = self.entries.iter().find(|e| e.id == id) else { return };
        let text = if e.meta.folder { format!("{}: {}", t("Folder"), e.meta.title) } else { format!("{}: {}      {}: {}", t("Title"), e.meta.title, t("Time"), store::format_duration(e.meta.seconds)) };
        egui::Panel::bottom("pc_g_strip").exact_size(34.0).frame(egui::Frame::NONE.fill(GALLERY_BG).inner_margin(Margin::symmetric(14, 4))).show(ui, |ui| {
            ui.painter().text(pos2(ui.max_rect().left(), ui.max_rect().center().y), Align2::LEFT_CENTER, text, FontId::proportional(15.0), Color32::BLACK);
        });
    }

    // ---- grid ------------------------------------------------------------------------------

    fn grid(&mut self, ui: &mut Ui, ctx: &Context, action: &mut Option<Action>) {
        let items = self.visible();
        if items.is_empty() {
            ui.centered_and_justified(|ui| {
                ui.label(egui::RichText::new(t("Tap + to start a new canvas.")).color(Color32::from_gray(0x70)).size(17.0));
            });
            return;
        }
        let gap = 8.0;
        let cols = 3usize;
        let cell_w = ((ui.available_width() - gap * (cols as f32 + 1.0)) / cols as f32).floor();
        egui::ScrollArea::vertical().auto_shrink([false, false]).show(ui, |ui| {
            ui.add_space(gap);
            for chunk in items.chunks(cols) {
                ui.horizontal(|ui| {
                    ui.add_space(gap);
                    ui.spacing_mut().item_spacing.x = gap;
                    for e in chunk {
                        self.cell(ui, ctx, e, cell_w, action);
                    }
                });
                ui.add_space(4.0);
            }
            ui.add_space(20.0);
        });
    }

    fn cell(&mut self, ui: &mut Ui, ctx: &Context, e: &Entry, w: f32, action: &mut Option<Action>) {
        let (rect, resp) = ui.allocate_exact_size(vec2(w, w + 30.0), Sense::click());
        let img = Rect::from_min_size(rect.min, vec2(w, w));
        let selected = if self.multi { self.picks.contains(&e.id) } else { self.selected.as_deref() == Some(e.id.as_str()) };
        if e.meta.folder {
            let body = Rect::from_min_max(pos2(img.left() + 6.0, img.top() + 22.0), pos2(img.right() - 6.0, img.bottom() - 10.0));
            ui.painter().rect_filled(Rect::from_min_size(pos2(body.left(), img.top() + 10.0), vec2(w * 0.4, 18.0)), 6.0, Color32::from_rgb(0xf0, 0xc1, 0x4b));
            ui.painter().rect_filled(body, 10.0, Color32::from_rgb(0xf7, 0xd0, 0x6a));
            ui.painter().text(body.center(), Align2::CENTER_CENTER, e.children.to_string(), FontId::proportional(26.0), Color32::from_rgb(0x7a, 0x55, 0x00));
        } else {
            ui.painter().rect_filled(img, 2.0, Color32::WHITE);
            if let Some(tex) = self.thumb(ctx, e) {
                let (tw, th) = (tex.size()[0] as f32, tex.size()[1] as f32);
                let k = (w / tw).min(w / th);
                let r = Rect::from_center_size(img.center(), vec2(tw * k, th * k));
                ui::checker(&ui.painter_at(r), r, 8.0);
                ui.painter().image(tex.id(), r, Rect::from_min_max(pos2(0.0, 0.0), pos2(1.0, 1.0)), Color32::WHITE);
            }
        }
        ui::stroke_rect(ui.painter(), img, 2.0, if selected { ACCENT } else { Color32::from_gray(0x9a) }, if selected { 4.0 } else { 1.5 });
        if self.multi {
            let c = pos2(img.right() - 16.0, img.top() + 16.0);
            ui.painter().circle_filled(c, 12.0, if selected { ACCENT } else { Color32::from_white_alpha(200) });
            ui.painter().circle_stroke(c, 12.0, Stroke::new(1.5, Color32::from_gray(0x60)));
            if selected {
                icons::paint(ui.painter(), Rect::from_center_size(c, vec2(18.0, 18.0)), Icon::Check, Color32::WHITE);
            }
        }
        let mut title = e.meta.title.clone();
        let max_chars = ((w / 8.2) as usize).max(6);
        if title.chars().count() > max_chars {
            title = title.chars().take(max_chars.saturating_sub(1)).collect::<String>() + "…";
        }
        ui.painter().text(pos2(rect.center().x, rect.bottom() - 12.0), Align2::CENTER_CENTER, title, FontId::proportional(15.0), Color32::BLACK);
        if resp.clicked() {
            if self.multi {
                if !self.picks.remove(&e.id) {
                    self.picks.insert(e.id.clone());
                }
            } else if e.meta.folder || self.selected.as_deref() == Some(e.id.as_str()) {
                self.open_item(&e.id, action);
            } else {
                self.selected = Some(e.id.clone());
            }
        }
    }

    // ---- popups ----------------------------------------------------------------------------

    fn popups(&mut self, ctx: &Context, action: &mut Option<Action>) {
        let screen = ctx.content_rect();
        if self.menu {
            let mut close = true;
            let ids = self.ids_for_action();
            let mut chosen = 0;
            egui::Area::new(Id::new("pc_g_menu")).order(egui::Order::Foreground).fixed_pos(pos2(screen.right() - 10.0, screen.bottom() - 70.0)).pivot(Align2::RIGHT_BOTTOM).show(ctx, |ui| {
                egui::Frame::new().fill(Color32::from_rgb(0x3a, 0x3d, 0x41)).corner_radius(16.0).inner_margin(Margin::same(8)).show(ui, |ui| {
                    ui.set_width((screen.width() * 0.82).min(380.0));
                    ui.vertical_centered(|ui| ui.label(egui::RichText::new(t("Please choose")).color(Color32::WHITE).size(17.0)));
                    ui.spacing_mut().item_spacing.y = 0.0;
                    egui::Frame::new().fill(SHEET).corner_radius(10.0).show(ui, |ui| {
                        if ui::row(ui, &t("Delete Permanently"), "", true, SHEET, Color32::BLACK).clicked() {
                            chosen = 1;
                        }
                        if ui::row(ui, &t("Recently Deleted"), "", false, SHEET, Color32::BLACK).clicked() {
                            chosen = 2;
                        }
                        if ui::row(ui, &t("Duplicate"), "", false, SHEET, Color32::BLACK).clicked() {
                            chosen = 3;
                        }
                        if ui::row(ui, &t("Save Image (PNG / JPEG / PSD)"), "", false, SHEET, Color32::BLACK).clicked() {
                            chosen = 4;
                        }
                        if ui::row(ui, &t("New Folder"), "", false, SHEET, Color32::BLACK).clicked() {
                            chosen = 5;
                        }
                        if ui::row(ui, &t("Show Storage Path"), "", false, SHEET, Color32::BLACK).clicked() {
                            chosen = 6;
                        }
                    });
                });
            });
            let outside = ctx.input(|i| i.pointer.primary_pressed()) && ctx.input(|i| i.pointer.latest_pos()).is_some_and(|p| ctx.layer_id_at(p).is_none_or(|l| l.order != egui::Order::Foreground));
            match chosen {
                1 if !ids.is_empty() => self.confirm = Some(Confirm::Purge(ids)),
                2 => {
                    self.bin = true;
                    self.loaded = false;
                }
                3 if !ids.is_empty() => {
                    for id in &ids {
                        if let Err(e) = self.store.duplicate(id, &format!(" {}", t("(copy)"))) {
                            toast(e);
                        }
                    }
                    self.loaded = false;
                }
                4 if !ids.is_empty() => self.export = Some(ExportDlg { ids, format: Format::Png, share: false }),
                5 => self.prompt = Some(Prompt::NewFolder(t("New Folder"))),
                6 => self.path_dialog = true,
                1 | 3 | 4 => toast(t("Select an artwork first.")),
                0 => close = outside,
                _ => {}
            }
            if close {
                self.menu = false;
            }
        }
        if self.edit_sheet {
            let ids = self.ids_for_action();
            let mut chosen = 0;
            let mut close = false;
            egui::Modal::new(Id::new("pc_g_edit_sheet")).show(ctx, |ui| {
                ui.set_width((screen.width() - 56.0).min(420.0));
                ui.label(egui::RichText::new(t("Edit")).size(19.0).strong());
                ui.spacing_mut().item_spacing.y = 0.0;
                for (i, (label, danger)) in [("Rename", false), ("Move to Folder", false), ("New Folder", false), ("Duplicate", false), ("Delete", true)].iter().enumerate() {
                    if ui::row(ui, &t(label), "", *danger, PANEL_DARK, TEXT).clicked() {
                        chosen = i + 1;
                    }
                }
                ui.add_space(8.0);
                if ui::text_button(ui, &t("Close"), PANEL, TEXT, 100.0).clicked() {
                    close = true;
                }
            });
            if chosen != 0 {
                close = true;
                match chosen {
                    1 => {
                        if let Some(id) = ids.first() {
                            self.prompt = Some(Prompt::Rename(id.clone(), self.title_of(id)));
                        }
                    }
                    2 => self.move_ids = Some(ids),
                    3 => self.prompt = Some(Prompt::NewFolder(t("New Folder"))),
                    4 => {
                        for id in &ids {
                            if let Err(e) = self.store.duplicate(id, &format!(" {}", t("(copy)"))) {
                                toast(e);
                            }
                        }
                        self.loaded = false;
                    }
                    _ => self.confirm = Some(Confirm::Trash(ids)),
                }
            }
            if close {
                self.edit_sheet = false;
            }
        }
        if self.new_sheet {
            self.new_sheet(ctx, action);
        }
    }

    fn new_sheet(&mut self, ctx: &Context, action: &mut Option<Action>) {
        let screen = ctx.content_rect();
        let mut close = false;
        let mut create: Option<(u32, u32, f32)> = None;
        let mut folder = false;
        let mut import = false;
        egui::Area::new(Id::new("pc_g_new")).order(egui::Order::Foreground).fixed_pos(screen.min).show(ctx, |ui| {
            egui::Frame::new().fill(SHEET).inner_margin(Margin::same(0)).show(ui, |ui| {
                ui.set_width(screen.width());
                ui.set_height(screen.height());
                let (r, _) = ui.allocate_exact_size(vec2(screen.width(), 54.0), Sense::hover());
                ui.painter().rect_filled(r, 0.0, Color32::from_rgb(0x44, 0x44, 0x46));
                ui.painter().text(pos2(r.left() + 14.0, r.center().y), Align2::LEFT_CENTER, t("New"), FontId::proportional(20.0), Color32::WHITE);
                let cr = Rect::from_center_size(pos2(r.right() - 28.0, r.center().y), vec2(48.0, 48.0));
                let resp = ui.interact(cr, Id::new("pc_g_new_x"), Sense::click());
                icons::paint(ui.painter(), cr.shrink(12.0), Icon::Close, Color32::WHITE);
                if resp.clicked() {
                    close = true;
                }
                egui::ScrollArea::vertical().auto_shrink([false, false]).show(ui, |ui| {
                    ui.spacing_mut().item_spacing.y = 0.0;
                    if ui::row(ui, &t("Folder"), "", false, SHEET, Color32::BLACK).clicked() {
                        folder = true;
                    }
                    if ui::row(ui, &t("Import Image"), "", false, SHEET, Color32::BLACK).clicked() {
                        import = true;
                    }
                    section(ui, &t("Illustration"));
                    // custom size
                    let (w, h) = (self.custom.0.clone(), self.custom.1.clone());
                    ui.horizontal(|ui| {
                        ui.add_space(14.0);
                        ui.label(egui::RichText::new(t("Custom")).size(17.0).color(Color32::BLACK));
                        ui.add_space(8.0);
                        ui.add(egui::TextEdit::singleline(&mut self.custom.0).desired_width(70.0).font(FontId::proportional(17.0)));
                        ui.label(egui::RichText::new("×").size(17.0).color(Color32::BLACK));
                        ui.add(egui::TextEdit::singleline(&mut self.custom.1).desired_width(70.0).font(FontId::proportional(17.0)));
                        if ui::text_button(ui, &t("Create"), ACCENT, Color32::WHITE, 70.0).clicked() {
                            if let (Ok(a), Ok(b)) = (self.custom.0.trim().parse::<u32>(), self.custom.1.trim().parse::<u32>()) {
                                create = Some((a, b, 72.0));
                            } else {
                                toast(t("Enter the width and height in pixels."));
                            }
                        }
                    });
                    let _ = (w, h);
                    for (label, _, w, h, dpi) in PRESETS {
                        if ui::row(ui, &t(label), &format!("{w} x {h}"), false, SHEET, Color32::BLACK).clicked() {
                            create = Some((*w, *h, *dpi));
                        }
                    }
                    section(ui, &t("For Printing"));
                    for (label, w, h, dpi) in PRINT_PRESETS {
                        if ui::row(ui, &t(label), &format!("{dpi:.0} dpi  {w} x {h}"), false, SHEET, Color32::BLACK).clicked() {
                            create = Some((*w, *h, *dpi));
                        }
                    }
                });
            });
        });
        if folder {
            self.prompt = Some(Prompt::NewFolder(t("New Folder")));
            close = true;
        }
        if import {
            *action = Some(Action::Import { parent: self.folder.clone() });
            close = true;
        }
        if let Some((w, h, dpi)) = create {
            if w == 0 || h == 0 || w > MAX_SIDE || h > MAX_SIDE || u64::from(w) * u64::from(h) > MAX_PIXELS {
                toast(format!("{} {} px", t("The canvas is too large. At most"), MAX_PIXELS));
            } else {
                *action = Some(Action::New { width: w, height: h, dpi, parent: self.folder.clone() });
                close = true;
            }
        }
        if close {
            self.new_sheet = false;
        }
    }

    // ---- dialogs ---------------------------------------------------------------------------

    fn dialogs(&mut self, ctx: &Context, _action: &mut Option<Action>) {
        if let Some(p) = self.prompt.take() {
            match p {
                Prompt::NewFolder(mut name) => match ui::text_dialog(ctx, "pc_g_p_folder", &t("New Folder"), &mut name, &t("Create")) {
                    ui::DialogResult::Open => self.prompt = Some(Prompt::NewFolder(name)),
                    ui::DialogResult::Ok => {
                        let name = if name.trim().is_empty() { t("New Folder") } else { name.trim().to_string() };
                        if let Err(e) = self.store.create_folder(&name, &self.folder) {
                            toast(e);
                        }
                        self.loaded = false;
                    }
                    ui::DialogResult::Cancel => {}
                },
                Prompt::Rename(id, mut name) => match ui::text_dialog(ctx, "pc_g_p_rename", &t("Rename"), &mut name, &t("OK")) {
                    ui::DialogResult::Open => self.prompt = Some(Prompt::Rename(id, name)),
                    ui::DialogResult::Ok => {
                        if !name.trim().is_empty() {
                            if let Err(e) = self.store.rename(&id, name.trim()) {
                                toast(e);
                            }
                            self.loaded = false;
                        }
                    }
                    ui::DialogResult::Cancel => {}
                },
            }
        }
        if let Some(c) = self.confirm.take() {
            let (title, body, label) = match &c {
                Confirm::Purge(ids) => (t("Delete Permanently"), format!("{} ({})", t("This cannot be undone."), ids.len()), t("Delete")),
                Confirm::Trash(ids) => (t("Delete"), format!("{} ({})", t("Move to Recently Deleted?"), ids.len()), t("Delete")),
                Confirm::EmptyBin => (t("Empty"), t("Delete everything in Recently Deleted for good?"), t("Empty")),
            };
            match ui::confirm_dialog(ctx, "pc_g_confirm", &title, &body, &label, true) {
                ui::DialogResult::Open => self.confirm = Some(c),
                ui::DialogResult::Ok => {
                    match c {
                        Confirm::Purge(ids) => {
                            for id in ids {
                                let _ = self.store.purge(&id);
                            }
                        }
                        Confirm::Trash(ids) => {
                            for id in ids {
                                if let Err(e) = self.store.trash(&id) {
                                    toast(e);
                                }
                            }
                        }
                        Confirm::EmptyBin => self.store.empty_bin(),
                    }
                    self.picks.clear();
                    self.selected = None;
                    self.loaded = false;
                }
                ui::DialogResult::Cancel => {}
            }
        }
        if self.info.is_some() {
            self.info_dialog(ctx);
        }
        if self.export.is_some() {
            self.export_dialog(ctx);
        }
        if self.move_ids.is_some() {
            self.move_dialog(ctx);
        }
        if self.path_dialog {
            let mut close = false;
            egui::Modal::new(Id::new("pc_g_path")).show(ctx, |ui| {
                ui.set_width((ctx.content_rect().width() - 56.0).min(420.0));
                ui.label(egui::RichText::new(t("Storage")).size(19.0).strong());
                let text = if platform::get().persistent() { t("Artworks are kept in PaintCraft's private storage on this device and saved automatically.") } else { t("Browser preview: artworks live in memory and are lost when you close the page.") };
                ui.label(egui::RichText::new(text).size(15.0));
                ui.add_space(8.0);
                if ui::text_button(ui, &t("Close"), ACCENT, Color32::WHITE, 100.0).clicked() {
                    close = true;
                }
            });
            if close {
                self.path_dialog = false;
            }
        }
    }

    fn info_dialog(&mut self, ctx: &Context) {
        let Some(mut info) = self.info.take() else { return };
        let Some(e) = self.entries.iter().find(|e| e.id == info.id).cloned() else { return };
        let off = platform::get().utc_offset_seconds();
        let id_lang = i18n::is_indonesian();
        let mut ok = false;
        let mut cancel = false;
        let width = (ctx.content_rect().width() - 28.0).min(480.0);
        let tex = self.thumb(ctx, &e);
        egui::Modal::new(Id::new("pc_g_info")).show(ctx, |ui| {
            ui.set_width(width);
            ui.horizontal(|ui| {
                ui.label(egui::RichText::new(t("Artwork Information")).size(20.0).strong());
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    if ui::bare_icon(ui, 40.0, Icon::Check, Color32::WHITE).clicked() {
                        ok = true;
                    }
                    if ui::bare_icon(ui, 40.0, Icon::Close, TEXT_DIM).clicked() {
                        cancel = true;
                    }
                });
            });
            egui::Frame::new().fill(PANEL_DARK).corner_radius(10.0).inner_margin(Margin::same(10)).show(ui, |ui| {
                ui.horizontal(|ui| {
                    let (r, _) = ui.allocate_exact_size(vec2(120.0, 120.0), Sense::hover());
                    ui.painter().rect_filled(r, 2.0, Color32::WHITE);
                    if let Some(tex) = &tex {
                        let (tw, th) = (tex.size()[0] as f32, tex.size()[1] as f32);
                        let k = (120.0 / tw).min(120.0 / th);
                        ui.painter().image(tex.id(), Rect::from_center_size(r.center(), vec2(tw * k, th * k)), Rect::from_min_max(pos2(0.0, 0.0), pos2(1.0, 1.0)), Color32::WHITE);
                    }
                    ui.vertical(|ui| {
                        let line = |ui: &mut Ui, k: &str, v: String| {
                            ui.label(egui::RichText::new(format!("{}: {}", t(k), v)).size(14.5).color(TEXT));
                        };
                        line(ui, "Canvas Size", format!("{}x{}", e.meta.width, e.meta.height));
                        line(ui, "File Size", store::format_bytes(e.meta.bytes));
                        line(ui, "Type", t("Illustration"));
                        line(ui, "Created", store::format_time(e.meta.created, off, id_lang));
                        line(ui, "Updated", store::format_time(e.meta.updated, off, id_lang));
                        line(ui, "Time", store::format_duration(e.meta.seconds));
                    });
                });
            });
            ui.label(egui::RichText::new(t("Title")).size(16.0).color(TEXT));
            ui.add(egui::TextEdit::singleline(&mut info.title).desired_width(f32::INFINITY).font(FontId::proportional(18.0)));
            ui.label(egui::RichText::new(t("Artist")).size(16.0).color(TEXT));
            ui.add(egui::TextEdit::singleline(&mut info.artist).desired_width(f32::INFINITY).font(FontId::proportional(18.0)));
            ui.horizontal(|ui| {
                ui.label(egui::RichText::new(t("Description")).size(16.0).color(TEXT));
                let left = 280usize.saturating_sub(info.description.chars().count());
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| ui.label(egui::RichText::new(format!("{left} {}", t("left"))).color(TEXT_DIM)));
            });
            ui.add(egui::TextEdit::multiline(&mut info.description).desired_width(f32::INFINITY).desired_rows(3).font(FontId::proportional(16.0)));
            if info.description.chars().count() > 280 {
                info.description = info.description.chars().take(280).collect();
            }
        });
        if ok {
            let (title, artist, description) = (info.title.trim().to_string(), info.artist.clone(), info.description.clone());
            let title = if title.is_empty() { e.meta.title.clone() } else { title };
            if let Err(err) = self.store.update_meta(&info.id, |m| {
                m.title = title;
                m.artist = artist;
                m.description = description;
            }) {
                toast(err);
            }
            self.loaded = false;
        } else if !cancel {
            self.info = Some(info);
        }
    }

    fn export_dialog(&mut self, ctx: &Context) {
        let Some(mut dlg) = self.export.take() else { return };
        let width = (ctx.content_rect().width() - 48.0).min(460.0);
        let mut go = false;
        let mut cancel = false;
        egui::Modal::new(Id::new("pc_g_export")).show(ctx, |ui| {
            ui.set_width(width);
            ui.label(egui::RichText::new(if dlg.share { t("Share") } else { t("Save Image") }).size(19.0).strong());
            ui.add_space(6.0);
            ui.horizontal_wrapped(|ui| {
                for f in Format::ALL {
                    if ui::text_button(ui, f.label(), if dlg.format == f { ACCENT } else { PANEL_DARK }, Color32::WHITE, 70.0).clicked() {
                        dlg.format = f;
                    }
                }
            });
            ui.add_space(8.0);
            ui.horizontal(|ui| {
                if ui::text_button(ui, &t("Cancel"), PANEL, TEXT, 100.0).clicked() {
                    cancel = true;
                }
                if ui::text_button(ui, &if dlg.share { t("Share") } else { t("Save") }, ACCENT, Color32::WHITE, 100.0).clicked() {
                    go = true;
                }
            });
        });
        if go {
            for id in &dlg.ids {
                let title = self.title_of(id);
                match self.store.load(id) {
                    Ok(doc) => match exporter::export(&self.store, &doc, &title, dlg.format, dlg.share) {
                        Ok(()) => {
                            if !dlg.share {
                                toast(t("Saved"));
                            }
                        }
                        Err(e) => toast(e),
                    },
                    Err(e) => toast(e),
                }
                if dlg.share {
                    break;
                }
            }
        } else if !cancel {
            self.export = Some(dlg);
        }
    }

    fn move_dialog(&mut self, ctx: &Context) {
        let Some(ids) = self.move_ids.take() else { return };
        let folders: Vec<Entry> = self.entries.iter().filter(|e| e.meta.folder && e.meta.deleted == 0 && !ids.contains(&e.id)).cloned().collect();
        let mut target: Option<String> = None;
        let mut cancel = false;
        egui::Modal::new(Id::new("pc_g_move")).show(ctx, |ui| {
            ui.set_width((ctx.content_rect().width() - 56.0).min(420.0));
            ui.label(egui::RichText::new(t("Move to Folder")).size(19.0).strong());
            ui.spacing_mut().item_spacing.y = 0.0;
            egui::ScrollArea::vertical().max_height(320.0).show(ui, |ui| {
                if ui::row(ui, &t("My Gallery"), "", false, PANEL_DARK, TEXT).clicked() {
                    target = Some(String::new());
                }
                for f in &folders {
                    if ui::row(ui, &f.meta.title, &f.children.to_string(), false, PANEL_DARK, TEXT).clicked() {
                        target = Some(f.id.clone());
                    }
                }
            });
            ui.add_space(8.0);
            if ui::text_button(ui, &t("Cancel"), PANEL, TEXT, 100.0).clicked() {
                cancel = true;
            }
        });
        if let Some(parent) = target {
            for id in &ids {
                if let Err(e) = self.store.move_to(id, &parent) {
                    toast(e);
                }
            }
            self.picks.clear();
            self.loaded = false;
        } else if !cancel {
            self.move_ids = Some(ids);
        }
    }

    // ---- recently deleted ------------------------------------------------------------------

    fn bin_view(&mut self, ui: &mut Ui, ctx: &Context) {
        let items = self.binned();
        let off = platform::get().utc_offset_seconds();
        egui::CentralPanel::default().frame(egui::Frame::NONE.fill(GALLERY_BG)).show(ui, |ui| {
            if items.is_empty() {
                ui.centered_and_justified(|ui| ui.label(egui::RichText::new(t("Nothing here.")).color(Color32::from_gray(0x70)).size(17.0)));
                return;
            }
            egui::ScrollArea::vertical().auto_shrink([false, false]).show(ui, |ui| {
                for e in &items {
                    let (rect, _) = ui.allocate_exact_size(vec2(ui.available_width(), 84.0), Sense::hover());
                    ui.painter().rect_filled(rect.shrink2(vec2(8.0, 3.0)), 8.0, Color32::WHITE);
                    let th = Rect::from_min_size(rect.min + vec2(14.0, 10.0), vec2(64.0, 64.0));
                    ui.painter().rect_filled(th, 2.0, Color32::from_gray(0xee));
                    if e.meta.folder {
                        icons::paint(ui.painter(), th.shrink(10.0), Icon::Folder, Color32::from_gray(0x70));
                    } else if let Some(tex) = self.thumb(ctx, e) {
                        let (tw, thh) = (tex.size()[0] as f32, tex.size()[1] as f32);
                        let k = (64.0 / tw).min(64.0 / thh);
                        ui.painter().image(tex.id(), Rect::from_center_size(th.center(), vec2(tw * k, thh * k)), Rect::from_min_max(pos2(0.0, 0.0), pos2(1.0, 1.0)), Color32::WHITE);
                    }
                    ui.painter().text(pos2(th.right() + 12.0, rect.top() + 28.0), Align2::LEFT_CENTER, &e.meta.title, FontId::proportional(16.5), Color32::BLACK);
                    ui.painter().text(pos2(th.right() + 12.0, rect.top() + 52.0), Align2::LEFT_CENTER, store::format_time(e.meta.deleted, off, i18n::is_indonesian()), FontId::proportional(13.0), Color32::from_gray(0x60));
                    let rr = Rect::from_center_size(pos2(rect.right() - 70.0, rect.center().y), vec2(52.0, 52.0));
                    let dr = Rect::from_center_size(pos2(rect.right() - 20.0, rect.center().y), vec2(52.0, 52.0));
                    let restore = ui.interact(rr, Id::new(("pc_g_rest", &e.id)), Sense::click());
                    let del = ui.interact(dr, Id::new(("pc_g_del", &e.id)), Sense::click());
                    icons::paint(ui.painter(), rr.shrink(14.0), Icon::Refresh, ACCENT);
                    icons::paint(ui.painter(), dr.shrink(14.0), Icon::Trash, DANGER);
                    if restore.clicked() {
                        if let Err(err) = self.store.restore(&e.id) {
                            toast(err);
                        }
                        self.loaded = false;
                    }
                    if del.clicked() {
                        self.confirm = Some(Confirm::Purge(vec![e.id.clone()]));
                    }
                }
            });
        });
    }
}

fn section(ui: &mut Ui, label: &str) {
    let (rect, _) = ui.allocate_exact_size(vec2(ui.available_width(), 30.0), Sense::hover());
    ui.painter().rect_filled(rect, 0.0, Color32::from_rgb(0x44, 0x44, 0x46));
    ui.painter().text(pos2(rect.left() + 14.0, rect.center().y), Align2::LEFT_CENTER, label, FontId::proportional(15.0), Color32::WHITE);
}
