//! PaintCraft for Android: PhotoCraft's engine behind an ibisPaint-style touch interface
//! (`paintcraft-ui`), compiled as a `cdylib` that a GameActivity (see `android/`) loads. There is
//! no web view: eframe draws with wgpu (Vulkan, OpenGL ES fallback) straight into the activity.
//!
//! This crate is only the Android shell: the JNI [`bridge`] (pickers, saving to Pictures, the
//! share sheet, the language), the [`platform`](paintcraft_ui::platform) implementation on top of
//! it, and the eframe host. Everything on screen lives in `paintcraft-ui`.
#![cfg(target_os = "android")]

mod bridge;

use std::path::PathBuf;
use std::sync::Arc;

use android_activity::AndroidApp;
use paintcraft_ui::PaintApp;
use paintcraft_ui::platform::{self, Picked, Platform};

struct Logger;

impl log::Log for Logger {
    fn enabled(&self, m: &log::Metadata) -> bool {
        m.level() <= log::Level::Info
    }

    fn log(&self, r: &log::Record) {
        if self.enabled(r.metadata()) {
            eprintln!("[{}] {}: {}", r.level(), r.target(), r.args());
        }
    }

    fn flush(&self) {}
}

struct AndroidPlatform {
    root: PathBuf,
    tmp: PathBuf,
}

impl Platform for AndroidPlatform {
    fn pick_picture(&self) {
        bridge::pick_picture();
    }

    fn take_picked(&self) -> Vec<Picked> {
        bridge::take_picked()
            .into_iter()
            .filter_map(|path| {
                let name = std::path::Path::new(&path).file_name()?.to_string_lossy().to_string();
                let bytes = std::fs::read(&path).ok()?;
                Some(Picked { name, bytes })
            })
            .collect()
    }

    fn save_to_device(&self, name: &str, mime: &str, bytes: &[u8]) -> bool {
        let path = self.tmp.join(name);
        std::fs::write(&path, bytes).is_ok() && bridge::save_to_pictures(&path.to_string_lossy(), name, mime)
    }

    fn share(&self, name: &str, mime: &str, bytes: &[u8]) {
        let path = self.tmp.join(name);
        if std::fs::write(&path, bytes).is_ok() {
            bridge::share_file(&path.to_string_lossy(), mime);
        }
    }

    fn utc_offset_seconds(&self) -> i32 {
        bridge::utc_offset_seconds()
    }

    fn locale_tag(&self) -> Option<String> {
        bridge::locale_tag()
    }

    fn haptic(&self) {
        bridge::haptic();
    }

    fn now_secs(&self) -> i64 {
        std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_secs() as i64).unwrap_or(0)
    }

    fn storage_root(&self) -> Option<PathBuf> {
        Some(self.root.clone())
    }
}

/// Stencil bits of the window's render pass.
const STENCIL_BITS: u8 = 8;

/// Entry point: android-activity calls this on its own thread, not the Java UI thread.
#[unsafe(no_mangle)]
fn android_main(app: AndroidApp) {
    bridge::init(&app);
    bridge::set_loading_progress(10);
    let _ = log::set_logger(&Logger);
    log::set_max_level(log::LevelFilter::Info);
    install_panic_hook();
    let data = app.internal_data_path().unwrap_or_else(|| PathBuf::from("/data/local/tmp"));
    let tmp = data.join("tmp");
    let _ = std::fs::create_dir_all(&tmp);
    let root = bridge::gallery_root().map(PathBuf::from).unwrap_or_else(|| data.join("gallery"));
    // SAFETY: first thing `android_main` does, before this app spawns any thread of its own.
    unsafe {
        std::env::set_var("HOME", &data);
        std::env::set_var("TMPDIR", &tmp);
    }
    platform::install(Box::new(AndroidPlatform { root, tmp }));
    bridge::set_loading_progress(35);
    let options = eframe::NativeOptions { android_app: Some(app.clone()), wgpu_options: wgpu_options(), stencil_buffer: STENCIL_BITS, ..Default::default() };
    let result = eframe::run_native(
        "PaintCraft",
        options,
        Box::new(move |cc| {
            match &cc.wgpu_render_state {
                Some(rs) => log::info!("graphics: {:?}", rs.adapter.get_info()),
                None => log::info!("graphics: no wgpu device"),
            }
            bridge::set_context(cc.egui_ctx.clone());
            bridge::set_loading_progress(70);
            let app = PaintApp::new(&cc.egui_ctx);
            Ok(Box::new(AndroidHost { app, loading_hidden: false }))
        }),
    );
    if let Err(e) = result {
        log::error!("PaintCraft could not start: {e}");
    }
}

fn install_panic_hook() {
    let previous = std::panic::take_hook();
    std::panic::set_hook(Box::new(move |info| {
        let backtrace = std::backtrace::Backtrace::force_capture();
        log::error!("PANIC on thread {:?}: {info}\n{backtrace}", std::thread::current().name());
        previous(info);
    }));
}

/// Ask the adapter for its own limits. Vulkan first, OpenGL ES as the fallback.
fn wgpu_options() -> eframe::WgpuConfiguration {
    use eframe::egui_wgpu::WgpuSetup;
    let mut config = eframe::WgpuConfiguration::default();
    if let WgpuSetup::CreateNew(setup) = &mut config.wgpu_setup {
        setup.instance_descriptor.backends = eframe::wgpu::Backends::VULKAN | eframe::wgpu::Backends::GL;
        setup.device_descriptor = Arc::new(|adapter| {
            let limits = adapter.limits();
            eframe::wgpu::DeviceDescriptor {
                label: Some("PaintCraft device"),
                required_limits: eframe::wgpu::Limits { max_texture_dimension_2d: limits.max_texture_dimension_2d.min(16384), max_buffer_size: limits.max_buffer_size, ..limits },
                ..Default::default()
            }
        });
    }
    config
}

struct AndroidHost {
    app: PaintApp,
    loading_hidden: bool,
}

impl eframe::App for AndroidHost {
    fn logic(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        // Pictures opened from other apps ("Open with PaintCraft").
        for path in bridge::take_opened() {
            if let (Some(name), Ok(bytes)) = (std::path::Path::new(&path).file_name(), std::fs::read(&path)) {
                self.app.import_bytes(ctx, &name.to_string_lossy(), &bytes, "");
            }
        }
        // Back: close the panel, leave the canvas, go up a folder; only then leave the app.
        let close = ctx.input(|i| i.viewport().close_requested());
        let escape = ctx.input(|i| i.key_pressed(egui::Key::Escape));
        if (close || escape) && self.app.back() && close {
            ctx.send_viewport_cmd(egui::ViewportCommand::CancelClose);
        }
    }

    fn ui(&mut self, ui: &mut egui::Ui, _frame: &mut eframe::Frame) {
        self.app.ui(ui);
        if !self.loading_hidden {
            self.loading_hidden = true;
            bridge::set_loading_progress(100);
            bridge::hide_loading();
        }
    }

    fn on_exit(&mut self) {
        self.app.on_exit();
    }

    fn persist_egui_memory(&self) -> bool {
        false
    }
}
