//! The whole app: the gallery, and the editor once an artwork is opened. Hosts (the Android
//! activity, the browser page) create one [`PaintApp`] and call [`PaintApp::ui`] every frame.

use egui::{Context, Ui};
use photocraft_engine::Session;
use serde_json::json;

use crate::editor::{Editor, Exit};
use crate::gallery::{Action, Gallery};
use crate::i18n::{self, t};
use crate::platform;
use crate::store::Store;
use crate::ui::{self, toast};

pub struct PaintApp {
    store: Store,
    gallery: Gallery,
    editor: Option<Editor>,
    import_into: Option<String>,
}

impl PaintApp {
    /// `platform::install` must have run.
    pub fn new(ctx: &Context) -> PaintApp {
        ui::setup(ctx);
        i18n::init(platform::get().locale_tag().as_deref());
        let store = Store::open(platform::get().storage_root());
        PaintApp { gallery: Gallery::new(store.clone()), store, editor: None, import_into: None }
    }

    pub fn ui(&mut self, ui: &mut Ui) {
        let ctx = ui.ctx().clone();
        self.poll_imports(&ctx);
        if let Some(ed) = &mut self.editor {
            if let Some(Exit::Gallery) = ed.ui(ui) {
                self.editor = None;
                self.gallery.refresh();
            }
        } else if let Some(action) = self.gallery.ui(ui) {
            self.handle(&ctx, action);
        }
        ui::show_toasts(&ctx);
    }

    /// The system Back button / Escape. Returns false when the app should close.
    pub fn back(&mut self) -> bool {
        if let Some(ed) = &mut self.editor {
            ed.back();
            true
        } else {
            self.gallery.back()
        }
    }

    pub fn on_exit(&mut self) {
        if let Some(ed) = &mut self.editor {
            ed.flush();
        }
    }

    /// Save now (the app is going to the background).
    pub fn flush(&mut self) {
        self.on_exit();
    }

    fn handle(&mut self, ctx: &Context, action: Action) {
        match action {
            Action::Open(id) => self.open(ctx, &id),
            Action::New { width, height, dpi, parent } => match self.create(width, height, dpi, &parent) {
                Ok(id) => self.open(ctx, &id),
                Err(e) => toast(e),
            },
            Action::Import { parent } => {
                self.import_into = Some(parent);
                platform::get().pick_picture();
            }
        }
    }

    fn open(&mut self, ctx: &Context, id: &str) {
        match Editor::open(ctx, self.store.clone(), id) {
            Ok(ed) => self.editor = Some(ed),
            Err(e) => toast(e),
        }
    }

    fn create(&mut self, width: u32, height: u32, dpi: f32, parent: &str) -> Result<String, String> {
        let title = format!("{} {}", t("Untitled"), self.store.next_number());
        let mut session = Session::new();
        session.execute("file.new", json!({"width": width, "height": height, "resolution": dpi, "name": title, "background": "white"})).map_err(|e| format!("{e}"))?;
        let doc = session.active().ok_or("no document")?.doc.clone();
        self.store.new_artwork(&doc, &title, parent)
    }

    /// A picture handed to the app (picked, or opened from another app) becomes a new artwork.
    pub fn import_bytes(&mut self, ctx: &Context, name: &str, bytes: &[u8], parent: &str) {
        let stem = name.rsplit_once('.').map(|(s, _)| s.to_string()).unwrap_or_else(|| name.to_string());
        match photocraft_io::import(name, bytes) {
            Ok(r) => {
                let doc = r.document;
                let px = u64::from(doc.size.width) * u64::from(doc.size.height);
                if px > crate::gallery::MAX_PIXELS || doc.size.width.max(doc.size.height) > crate::gallery::MAX_SIDE {
                    toast(t("That picture is too large to paint on a phone."));
                    return;
                }
                let title = if stem.is_empty() { format!("{} {}", t("Untitled"), self.store.next_number()) } else { stem };
                match self.store.new_artwork(&doc, &title, parent) {
                    Ok(id) => self.open(ctx, &id),
                    Err(e) => toast(e),
                }
            }
            Err(e) => toast(format!("{e}")),
        }
    }

    fn poll_imports(&mut self, ctx: &Context) {
        let Some(parent) = self.import_into.clone() else { return };
        let picked = platform::get().take_picked();
        if picked.is_empty() {
            return;
        }
        self.import_into = None;
        for p in picked {
            self.import_bytes(ctx, &p.name, &p.bytes, &parent);
        }
    }
}
