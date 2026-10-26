//! The gallery on disk: artworks and folders, thumbnails, the recently-deleted bin.
//!
//! Layout (`<root>/items/<id>/`): every artwork and every folder is one directory with a
//! `meta.json`; an artwork also has `work.pcraft` (the layered document) and `thumb.rgba`
//! (u32 width, u32 height, RGBA bytes). A folder's contents are the items whose `parent` is its
//! id, so moving an item is a metadata change. Deleting moves an item to the bin (`deleted` is
//! the time it went there); the bin is emptied by the user.

use std::collections::BTreeMap;
use std::path::PathBuf;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex, PoisonError};

use photocraft_doc::Document;
use serde::{Deserialize, Serialize};

use crate::platform;

#[derive(Clone, Debug, Default, Serialize, Deserialize, PartialEq)]
#[serde(default)]
pub struct Meta {
    pub title: String,
    pub folder: bool,
    pub parent: String,
    pub created: i64,
    pub updated: i64,
    pub width: u32,
    pub height: u32,
    pub seconds: u64,
    pub artist: String,
    pub description: String,
    pub deleted: i64,
    pub bytes: u64,
}

#[derive(Clone, Debug)]
pub struct Entry {
    pub id: String,
    pub meta: Meta,
    /// Folders: how many live items they hold.
    pub children: usize,
}

#[derive(Clone, Debug)]
pub struct Thumb {
    pub width: u32,
    pub height: u32,
    pub rgba: Vec<u8>,
}

/// Disk on Android; memory in the browser preview.
#[derive(Clone)]
enum Backend {
    Disk(PathBuf),
    Memory(Arc<Mutex<BTreeMap<String, Vec<u8>>>>),
}

#[derive(Clone)]
pub struct Store {
    backend: Backend,
}

static COUNTER: AtomicU64 = AtomicU64::new(0);

pub fn now() -> i64 {
    platform::now()
}

fn new_id() -> String {
    let ms = platform::get().now_secs().max(0) as u64;
    format!("{:09x}{:05x}", ms, COUNTER.fetch_add(1, Ordering::Relaxed) % 0xfffff)
}

impl Backend {
    fn read(&self, rel: &str) -> Option<Vec<u8>> {
        match self {
            Backend::Disk(root) => std::fs::read(root.join(rel)).ok(),
            Backend::Memory(m) => m.lock().unwrap_or_else(PoisonError::into_inner).get(rel).cloned(),
        }
    }

    fn write(&self, rel: &str, bytes: &[u8]) -> Result<(), String> {
        match self {
            Backend::Disk(root) => {
                let path = root.join(rel);
                if let Some(dir) = path.parent() {
                    std::fs::create_dir_all(dir).map_err(|e| e.to_string())?;
                }
                let tmp = path.with_extension("tmp");
                std::fs::write(&tmp, bytes).map_err(|e| e.to_string())?;
                std::fs::rename(&tmp, &path).map_err(|e| e.to_string())
            }
            Backend::Memory(m) => {
                m.lock().unwrap_or_else(PoisonError::into_inner).insert(rel.to_string(), bytes.to_vec());
                Ok(())
            }
        }
    }

    /// Ids of the directories under `items/`.
    fn item_ids(&self) -> Vec<String> {
        match self {
            Backend::Disk(root) => std::fs::read_dir(root.join("items")).map(|rd| rd.flatten().map(|e| e.file_name().to_string_lossy().to_string()).collect()).unwrap_or_default(),
            Backend::Memory(m) => {
                let map = m.lock().unwrap_or_else(PoisonError::into_inner);
                let mut ids: Vec<String> = map.keys().filter_map(|k| k.strip_prefix("items/")).filter_map(|r| r.split('/').next()).map(str::to_string).collect();
                ids.dedup();
                ids
            }
        }
    }

    fn remove_item(&self, id: &str) {
        match self {
            Backend::Disk(root) => {
                let _ = std::fs::remove_dir_all(root.join("items").join(id));
            }
            Backend::Memory(m) => {
                let prefix = format!("items/{id}/");
                m.lock().unwrap_or_else(PoisonError::into_inner).retain(|k, _| !k.starts_with(&prefix));
            }
        }
    }
}

fn key(id: &str, file: &str) -> String {
    format!("items/{id}/{file}")
}

impl Store {
    pub fn open(root: Option<PathBuf>) -> Store {
        match root {
            Some(r) => {
                let _ = std::fs::create_dir_all(r.join("items"));
                Store { backend: Backend::Disk(r) }
            }
            None => Store { backend: Backend::Memory(Arc::default()) },
        }
    }

    pub fn meta(&self, id: &str) -> Option<Meta> {
        let bytes = self.backend.read(&key(id, "meta.json"))?;
        serde_json::from_slice(&bytes).ok()
    }

    fn write_meta(&self, id: &str, meta: &Meta) -> Result<(), String> {
        let text = serde_json::to_vec(meta).map_err(|e| e.to_string())?;
        self.backend.write(&key(id, "meta.json"), &text)
    }

    pub fn update_meta(&self, id: &str, f: impl FnOnce(&mut Meta)) -> Result<(), String> {
        let mut meta = self.meta(id).ok_or("missing item")?;
        f(&mut meta);
        self.write_meta(id, &meta)
    }

    /// Every item, newest first.
    pub fn scan(&self) -> Vec<Entry> {
        let mut out = Vec::new();
        for id in self.backend.item_ids() {
            if let Some(meta) = self.meta(&id) {
                out.push(Entry { id, meta, children: 0 });
            }
        }
        let live: Vec<(String, bool)> = out.iter().map(|e| (e.meta.parent.clone(), e.meta.deleted == 0)).collect();
        for e in out.iter_mut().filter(|e| e.meta.folder) {
            e.children = live.iter().filter(|(p, ok)| *ok && *p == e.id).count();
        }
        out.sort_by(|a, b| b.meta.updated.cmp(&a.meta.updated).then_with(|| b.id.cmp(&a.id)));
        out
    }

    /// The next default title number ("Untitled 78").
    pub fn next_number(&self) -> u64 {
        let n = self.backend.read("counter").and_then(|b| String::from_utf8(b).ok()).and_then(|s| s.trim().parse::<u64>().ok()).unwrap_or(0) + 1;
        let _ = self.backend.write("counter", n.to_string().as_bytes());
        n
    }

    pub fn create_folder(&self, name: &str, parent: &str) -> Result<String, String> {
        let id = new_id();
        let t = now();
        let meta = Meta { title: name.to_string(), folder: true, parent: parent.to_string(), created: t, updated: t, ..Meta::default() };
        self.write_meta(&id, &meta)?;
        Ok(id)
    }

    /// Store a new artwork made from `doc`.
    pub fn new_artwork(&self, doc: &Document, title: &str, parent: &str) -> Result<String, String> {
        let id = new_id();
        let t = now();
        let meta = Meta { title: title.to_string(), parent: parent.to_string(), created: t, updated: t, width: doc.size.width, height: doc.size.height, ..Meta::default() };
        self.write_meta(&id, &meta)?;
        self.save(&id, doc, 0)?;
        Ok(id)
    }

    /// Write the document, its thumbnail and the updated metadata.
    pub fn save(&self, id: &str, doc: &Document, add_seconds: u64) -> Result<(), String> {
        let opts = photocraft_format::SaveOptions { thumbnail: None, composite: None };
        let bytes = photocraft_format::save_to_bytes(doc, &opts).map_err(|e| format!("{e:?}"))?;
        self.backend.write(&key(id, "work.pcraft"), &bytes)?;
        let thumb = photocraft_compose::thumbnail(doc, 320);
        let mut raw = Vec::with_capacity(8 + thumb.pixels.len());
        raw.extend_from_slice(&thumb.width.to_le_bytes());
        raw.extend_from_slice(&thumb.height.to_le_bytes());
        raw.extend_from_slice(&thumb.pixels);
        self.backend.write(&key(id, "thumb.rgba"), &raw)?;
        let size = bytes.len() as u64;
        let (w, h) = (doc.size.width, doc.size.height);
        self.update_meta(id, |m| {
            m.updated = now();
            m.width = w;
            m.height = h;
            m.bytes = size;
            m.seconds += add_seconds;
        })
    }

    pub fn load(&self, id: &str) -> Result<Document, String> {
        let bytes = self.backend.read(&key(id, "work.pcraft")).ok_or("the artwork file is missing")?;
        photocraft_format::load_from_bytes(&bytes).map_err(|e| format!("{e:?}"))
    }

    pub fn thumb(&self, id: &str) -> Option<Thumb> {
        let raw = self.backend.read(&key(id, "thumb.rgba"))?;
        let w = u32::from_le_bytes(raw.get(0..4)?.try_into().ok()?);
        let h = u32::from_le_bytes(raw.get(4..8)?.try_into().ok()?);
        let rgba = raw.get(8..)?.to_vec();
        if (w as usize).checked_mul(h as usize)?.checked_mul(4)? != rgba.len() {
            return None;
        }
        Some(Thumb { width: w, height: h, rgba })
    }

    pub fn rename(&self, id: &str, title: &str) -> Result<(), String> {
        let title = title.to_string();
        self.update_meta(id, |m| m.title = title)
    }

    pub fn move_to(&self, id: &str, parent: &str) -> Result<(), String> {
        if id == parent {
            return Ok(());
        }
        let parent = parent.to_string();
        self.update_meta(id, |m| m.parent = parent)
    }

    pub fn duplicate(&self, id: &str, suffix: &str) -> Result<String, String> {
        let meta = self.meta(id).ok_or("missing item")?;
        if meta.folder {
            return Err("folders cannot be duplicated".into());
        }
        let new = new_id();
        for f in ["work.pcraft", "thumb.rgba"] {
            if let Some(bytes) = self.backend.read(&key(id, f)) {
                self.backend.write(&key(&new, f), &bytes)?;
            }
        }
        let t = now();
        let mut m = meta;
        m.title = format!("{}{}", m.title, suffix);
        m.created = t;
        m.updated = t;
        self.write_meta(&new, &m)?;
        Ok(new)
    }

    /// Move to the bin (a folder takes its contents along).
    pub fn trash(&self, id: &str) -> Result<(), String> {
        let t = now();
        let all = self.scan();
        let mut ids = vec![id.to_string()];
        let mut i = 0;
        while i < ids.len() {
            let cur = ids[i].clone();
            ids.extend(all.iter().filter(|e| e.meta.parent == cur && e.meta.deleted == 0).map(|e| e.id.clone()));
            i += 1;
        }
        for x in ids {
            self.update_meta(&x, |m| m.deleted = t)?;
        }
        Ok(())
    }

    /// Take an item out of the bin. A folder brings back what was deleted with it; an item whose
    /// folder is still in the bin goes back to the top level.
    pub fn restore(&self, id: &str) -> Result<(), String> {
        let all = self.scan();
        let me = all.iter().find(|e| e.id == id).ok_or("missing item")?;
        let stamp = me.meta.deleted;
        let parent_gone = all.iter().any(|e| e.id == me.meta.parent && e.meta.deleted != 0);
        let kids: Vec<String> = if me.meta.folder { all.iter().filter(|e| e.meta.parent == id && e.meta.deleted == stamp).map(|e| e.id.clone()).collect() } else { Vec::new() };
        self.update_meta(id, |m| {
            m.deleted = 0;
            if parent_gone {
                m.parent.clear();
            }
        })?;
        for k in kids {
            self.update_meta(&k, |m| m.deleted = 0)?;
        }
        Ok(())
    }

    /// Delete for good (a folder's contents too).
    pub fn purge(&self, id: &str) -> Result<(), String> {
        let all = self.scan();
        let kids: Vec<String> = all.iter().filter(|e| e.meta.parent == id).map(|e| e.id.clone()).collect();
        for k in kids {
            self.backend.remove_item(&k);
        }
        self.backend.remove_item(id);
        Ok(())
    }

    pub fn empty_bin(&self) {
        for e in self.scan().into_iter().filter(|e| e.meta.deleted != 0) {
            self.backend.remove_item(&e.id);
        }
    }
}

/// "25 Okt 2026 14.43.42"-style local time for the info sheet.
pub fn format_time(unix: i64, offset_secs: i32, indonesian: bool) -> String {
    use chrono::{Datelike, Timelike};
    let Some(dt) = chrono::DateTime::from_timestamp(unix + i64::from(offset_secs), 0) else { return String::new() };
    const EN: [&str; 12] = ["Jan", "Feb", "Mar", "Apr", "May", "Jun", "Jul", "Aug", "Sep", "Oct", "Nov", "Dec"];
    const ID: [&str; 12] = ["Jan", "Feb", "Mar", "Apr", "Mei", "Jun", "Jul", "Agu", "Sep", "Okt", "Nov", "Des"];
    let months = if indonesian { ID } else { EN };
    let month = months.get(dt.month0() as usize).copied().unwrap_or("");
    let sep = if indonesian { '.' } else { ':' };
    format!("{} {} {} {:02}{sep}{:02}{sep}{:02}", dt.day(), month, dt.year(), dt.hour(), dt.minute(), dt.second())
}

pub fn format_bytes(b: u64) -> String {
    if b >= 1 << 20 { format!("{:.2}MB", b as f64 / (1u64 << 20) as f64) } else { format!("{:.0}KB", (b as f64 / 1024.0).max(1.0)) }
}

pub fn format_duration(secs: u64) -> String {
    format!("{}:{:02}", secs / 60, secs % 60)
}
