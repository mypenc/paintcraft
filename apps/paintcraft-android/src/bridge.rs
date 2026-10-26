//! JNI bridge to the Kotlin half (`android/app/src/main/kotlin/…`).
//!
//! Rust → Kotlin: methods on `MainActivity`. Kotlin → Rust: the
//! `Java_ai_storyteller_paintcraft_NativeBridge_*` functions below.

use std::sync::{Mutex, OnceLock, PoisonError};
use std::sync::atomic::{AtomicI64, Ordering};

use android_activity::AndroidApp;
use jni::objects::{JClass, JObject, JString, JValue};
use jni::sys::jlong;
use jni::{JNIEnv, JavaVM};

static APP: OnceLock<AndroidApp> = OnceLock::new();
static CTX: OnceLock<egui::Context> = OnceLock::new();
static NEXT_ID: AtomicI64 = AtomicI64::new(1);
/// Pictures chosen in the system picker (copied into the app's folder), for the UI thread.
static PICKED: Mutex<Vec<String>> = Mutex::new(Vec::new());
/// Documents opened from other apps ("Open with").
static OPENED: Mutex<Vec<String>> = Mutex::new(Vec::new());
static PICKER_OPEN: Mutex<Option<i64>> = Mutex::new(None);

fn lock<T>(m: &Mutex<T>) -> std::sync::MutexGuard<'_, T> {
    m.lock().unwrap_or_else(PoisonError::into_inner)
}

pub fn init(app: &AndroidApp) {
    let _ = APP.set(app.clone());
}

pub fn set_context(ctx: egui::Context) {
    let _ = CTX.set(ctx);
}

fn wake() {
    if let Some(ctx) = CTX.get() {
        ctx.request_repaint();
    }
}

/// Run `f` on a thread attached to the JVM with the activity as `this`.
fn with_activity<R>(f: impl FnOnce(&mut JNIEnv, &JObject) -> jni::errors::Result<R>) -> Option<R> {
    let app = APP.get()?;
    // SAFETY: android-activity guarantees both pointers stay valid for the life of the app.
    let vm = unsafe { JavaVM::from_raw(app.vm_as_ptr().cast()) }.ok()?;
    let activity = unsafe { JObject::from_raw(app.activity_as_ptr().cast()) };
    let mut env = vm.attach_current_thread().ok()?;
    match f(&mut env, &activity) {
        Ok(v) => Some(v),
        Err(e) => {
            log::warn!("JNI call failed: {e}");
            let _ = env.exception_clear();
            None
        }
    }
}

fn string_result(env: &mut JNIEnv, value: JObject) -> jni::errors::Result<Option<String>> {
    if value.as_raw().is_null() {
        return Ok(None);
    }
    let value = JString::from(value);
    let text = env.get_string(&value)?;
    Ok(Some(String::from(text)))
}

/// Start the system picture picker. The answer arrives in [`take_picked`]. One picker at a time.
pub fn pick_picture() {
    {
        let mut open = lock(&PICKER_OPEN);
        if open.is_some() {
            return;
        }
        *open = Some(NEXT_ID.fetch_add(1, Ordering::Relaxed));
    }
    let id = lock(&PICKER_OPEN).unwrap_or(0);
    let started = with_activity(|env, activity| {
        let arg: JString = env.new_string("picture")?;
        env.call_method(activity, "openDialog", "(JLjava/lang/String;)V", &[JValue::Long(id), JValue::Object(&arg)]).map(|_| ())
    });
    if started.is_none() {
        *lock(&PICKER_OPEN) = None;
    }
}

/// Pictures chosen since the last call.
pub fn take_picked() -> Vec<String> {
    std::mem::take(&mut *lock(&PICKED))
}

/// Documents other apps asked PaintCraft to open since the last call.
pub fn take_opened() -> Vec<String> {
    std::mem::take(&mut *lock(&OPENED))
}

/// The gallery's folder inside the app's private storage.
pub fn gallery_root() -> Option<String> {
    with_activity(|env, activity| {
        let v = env.call_method(activity, "galleryRoot", "()Ljava/lang/String;", &[])?.l()?;
        string_result(env, v)
    })
    .flatten()
}

/// Copy a file into Pictures/PaintCraft (visible in the device's gallery).
pub fn save_to_pictures(path: &str, name: &str, mime: &str) -> bool {
    with_activity(|env, activity| {
        let p: JString = env.new_string(path)?;
        let n: JString = env.new_string(name)?;
        let m: JString = env.new_string(mime)?;
        env.call_method(
            activity,
            "saveToPictures",
            "(Ljava/lang/String;Ljava/lang/String;Ljava/lang/String;)Z",
            &[JValue::Object(&p), JValue::Object(&n), JValue::Object(&m)],
        )?
        .z()
    })
    .unwrap_or(false)
}

/// Open the system share sheet for a file.
pub fn share_file(path: &str, mime: &str) {
    with_activity(|env, activity| {
        let p: JString = env.new_string(path)?;
        let m: JString = env.new_string(mime)?;
        env.call_method(activity, "shareFile", "(Ljava/lang/String;Ljava/lang/String;)V", &[JValue::Object(&p), JValue::Object(&m)]).map(|_| ())
    });
}

pub fn haptic() {
    with_activity(|env, activity| env.call_method(activity, "haptic", "()V", &[]).map(|_| ()));
}

pub fn set_fullscreen(on: bool) {
    with_activity(|env, activity| env.call_method(activity, "setFullscreen", "(Z)V", &[JValue::Bool(u8::from(on))]).map(|_| ()));
}

pub fn set_loading_progress(percent: i32) {
    with_activity(|env, activity| env.call_method(activity, "setLoadingProgress", "(I)V", &[JValue::Int(percent)]).map(|_| ()));
}

pub fn hide_loading() {
    with_activity(|env, activity| env.call_method(activity, "hideLoading", "()V", &[]).map(|_| ()));
}

/// The device's language as a BCP 47 tag ("id-ID").
pub fn locale_tag() -> Option<String> {
    with_activity(|env, activity| {
        let v = env.call_method(activity, "localeTag", "()Ljava/lang/String;", &[])?.l()?;
        string_result(env, v)
    })
    .flatten()
}

/// Seconds east of UTC at this moment.
pub fn utc_offset_seconds() -> i32 {
    with_activity(|env, activity| env.call_method(activity, "utcOffsetSeconds", "()I", &[])?.i()).unwrap_or(0)
}

fn read_string(env: &mut JNIEnv, s: &JString) -> Option<String> {
    if s.as_raw().is_null() {
        return None;
    }
    env.get_string(s).ok().map(String::from)
}

/// `NativeBridge.onDialogAnswer(id: Long, path: String?)`.
#[unsafe(no_mangle)]
pub extern "system" fn Java_ai_storyteller_paintcraft_NativeBridge_onDialogAnswer(mut env: JNIEnv, _class: JClass, id: jlong, path: JString) {
    let answer = read_string(&mut env, &path);
    {
        let mut open = lock(&PICKER_OPEN);
        if *open == Some(id) {
            *open = None;
        }
    }
    if let Some(path) = answer {
        lock(&PICKED).push(path);
    }
    wake();
}

/// `NativeBridge.onOpenIntent(path: String)`: a picture opened from another app.
#[unsafe(no_mangle)]
pub extern "system" fn Java_ai_storyteller_paintcraft_NativeBridge_onOpenIntent(mut env: JNIEnv, _class: JClass, path: JString) {
    if let Some(p) = read_string(&mut env, &path) {
        lock(&OPENED).push(p);
        wake();
    }
}
