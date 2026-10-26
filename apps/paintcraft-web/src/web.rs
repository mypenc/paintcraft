//! The browser's half of `paintcraft_ui::platform::Platform`, and the eframe web runner.

use std::path::PathBuf;
use std::sync::{Mutex, OnceLock, PoisonError};

use paintcraft_ui::PaintApp;
use paintcraft_ui::platform::{self, Picked, Platform};
use wasm_bindgen::JsCast;
use wasm_bindgen::closure::Closure;

const CANVAS_ID: &str = "paintcraft_canvas";

static PICKED: Mutex<Vec<Picked>> = Mutex::new(Vec::new());
static CTX: OnceLock<egui::Context> = OnceLock::new();

struct WebPlatform;

fn download(name: &str, mime: &str, bytes: &[u8]) -> bool {
    let run = || -> Result<(), wasm_bindgen::JsValue> {
        let array = js_sys::Uint8Array::from(bytes);
        let parts = js_sys::Array::new();
        parts.push(&array);
        let opts = web_sys::BlobPropertyBag::new();
        opts.set_type(mime);
        let blob = web_sys::Blob::new_with_u8_array_sequence_and_options(&parts, &opts)?;
        let url = web_sys::Url::create_object_url_with_blob(&blob)?;
        let document = web_sys::window().and_then(|w| w.document()).ok_or("no document")?;
        let a: web_sys::HtmlAnchorElement = document.create_element("a")?.dyn_into()?;
        a.set_href(&url);
        a.set_download(name);
        a.click();
        let _ = web_sys::Url::revoke_object_url(&url);
        Ok(())
    };
    run().is_ok()
}

impl Platform for WebPlatform {
    fn pick_picture(&self) {
        let run = || -> Result<(), wasm_bindgen::JsValue> {
            let document = web_sys::window().and_then(|w| w.document()).ok_or("no document")?;
            let input: web_sys::HtmlInputElement = document.create_element("input")?.dyn_into()?;
            input.set_type("file");
            input.set_accept("image/*");
            let target = input.clone();
            let on_change = Closure::<dyn FnMut(web_sys::Event)>::new(move |_e: web_sys::Event| {
                let Some(file) = target.files().and_then(|f| f.get(0)) else { return };
                let name = file.name();
                wasm_bindgen_futures::spawn_local(async move {
                    if let Ok(buf) = wasm_bindgen_futures::JsFuture::from(file.array_buffer()).await {
                        let bytes = js_sys::Uint8Array::new(&buf).to_vec();
                        PICKED.lock().unwrap_or_else(PoisonError::into_inner).push(Picked { name, bytes });
                        if let Some(ctx) = CTX.get() {
                            ctx.request_repaint();
                        }
                    }
                });
            });
            input.add_event_listener_with_callback("change", on_change.as_ref().unchecked_ref())?;
            on_change.forget();
            input.click();
            Ok(())
        };
        let _ = run();
    }

    fn take_picked(&self) -> Vec<Picked> {
        std::mem::take(&mut *PICKED.lock().unwrap_or_else(PoisonError::into_inner))
    }

    fn save_to_device(&self, name: &str, mime: &str, bytes: &[u8]) -> bool {
        download(name, mime, bytes)
    }

    fn share(&self, name: &str, mime: &str, bytes: &[u8]) {
        download(name, mime, bytes);
    }

    fn utc_offset_seconds(&self) -> i32 {
        (-js_sys::Date::new_0().get_timezone_offset() * 60.0) as i32
    }

    fn locale_tag(&self) -> Option<String> {
        web_sys::window().and_then(|w| w.navigator().language())
    }

    fn now_secs(&self) -> i64 {
        (js_sys::Date::now() / 1000.0) as i64
    }

    fn storage_root(&self) -> Option<PathBuf> {
        None
    }
}

struct WebHost {
    app: PaintApp,
}

impl eframe::App for WebHost {
    fn logic(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        if ctx.input(|i| i.key_pressed(egui::Key::Escape)) {
            self.app.back();
        }
    }

    fn ui(&mut self, ui: &mut egui::Ui, _frame: &mut eframe::Frame) {
        self.app.ui(ui);
    }

    fn persist_egui_memory(&self) -> bool {
        false
    }
}

pub fn start() {
    std::panic::set_hook(Box::new(|info| web_sys::console::error_1(&format!("PaintCraft panicked: {info}").into())));
    wasm_bindgen_futures::spawn_local(async {
        let Some(document) = web_sys::window().and_then(|w| w.document()) else { return };
        let Some(canvas) = document.get_element_by_id(CANVAS_ID).and_then(|e| e.dyn_into::<web_sys::HtmlCanvasElement>().ok()) else {
            log::error!("missing <canvas id=\"{CANVAS_ID}\">");
            return;
        };
        let result = eframe::WebRunner::new()
            .start(
                canvas,
                eframe::WebOptions::default(),
                Box::new(|cc| {
                    platform::install(Box::new(WebPlatform));
                    let _ = CTX.set(cc.egui_ctx.clone());
                    Ok(Box::new(WebHost { app: PaintApp::new(&cc.egui_ctx) }))
                }),
            )
            .await;
        if let Some(el) = document.get_element_by_id("paintcraft_loading") {
            match result {
                Ok(()) => el.remove(),
                Err(e) => el.set_inner_html(&format!("<p>PaintCraft failed to start: {e:?}</p><p>A browser with WebGPU or WebGL2 is required.</p>")),
            }
        }
    });
}
