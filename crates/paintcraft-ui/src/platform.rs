//! What the host (Android activity or browser page) provides: pickers, saving to the device,
//! sharing, the clock and the language. The UI only talks to this trait.

use std::path::PathBuf;
use std::sync::OnceLock;

/// A picture the user chose.
pub struct Picked {
    pub name: String,
    pub bytes: Vec<u8>,
}

pub trait Platform: Send + Sync {
    /// Open the system picture picker; the result shows up in [`Platform::take_picked`].
    fn pick_picture(&self);
    fn take_picked(&self) -> Vec<Picked>;
    /// Put a file where the user can find it (Pictures on Android, a download in the browser).
    fn save_to_device(&self, name: &str, mime: &str, bytes: &[u8]) -> bool;
    fn share(&self, name: &str, mime: &str, bytes: &[u8]);
    fn utc_offset_seconds(&self) -> i32;
    fn locale_tag(&self) -> Option<String>;
    fn haptic(&self) {}
    fn now_secs(&self) -> i64;
    /// Where the gallery lives; `None` keeps it in memory (the browser preview).
    fn storage_root(&self) -> Option<PathBuf>;
    /// Whether work survives closing the app.
    fn persistent(&self) -> bool {
        self.storage_root().is_some()
    }
}

static PLATFORM: OnceLock<Box<dyn Platform>> = OnceLock::new();

pub fn install(p: Box<dyn Platform>) {
    let _ = PLATFORM.set(p);
}

pub fn get() -> &'static dyn Platform {
    PLATFORM.get().map(|b| b.as_ref()).expect("platform installed before the UI starts")
}

pub fn now() -> i64 {
    get().now_secs()
}
