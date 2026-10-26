//! Save a picture to the device (Pictures/PaintCraft) or hand it to the share sheet.

use photocraft_doc::Document;

use crate::platform;
use crate::store::Store;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Format {
    Png,
    Jpeg,
    Psd,
}

impl Format {
    pub const ALL: [Format; 3] = [Format::Png, Format::Jpeg, Format::Psd];

    pub fn ext(self) -> &'static str {
        match self {
            Format::Png => "png",
            Format::Jpeg => "jpg",
            Format::Psd => "psd",
        }
    }

    pub fn mime(self) -> &'static str {
        match self {
            Format::Png => "image/png",
            Format::Jpeg => "image/jpeg",
            Format::Psd => "image/vnd.adobe.photoshop",
        }
    }

    pub fn label(self) -> &'static str {
        match self {
            Format::Png => "PNG",
            Format::Jpeg => "JPEG",
            Format::Psd => "PSD (layers)",
        }
    }
}

fn safe_name(s: &str) -> String {
    let cleaned: String = s.chars().map(|c| if c.is_alphanumeric() || c == '-' || c == '_' { c } else { '_' }).collect();
    if cleaned.trim_matches('_').is_empty() { "artwork".into() } else { cleaned }
}

/// Encode `doc` and save it to the device, or open the share sheet when `share`.
pub fn export(_store: &Store, doc: &Document, title: &str, format: Format, share: bool) -> Result<(), String> {
    let result = photocraft_io::export(doc, format.ext(), &photocraft_io::ExportOptions::default()).map_err(|e| format!("{e}"))?;
    let name = format!("{}.{}", safe_name(title), format.ext());
    let p = platform::get();
    if share {
        p.share(&name, format.mime(), &result.bytes);
        Ok(())
    } else if p.save_to_device(&name, format.mime(), &result.bytes) {
        Ok(())
    } else {
        Err("could not save the picture".into())
    }
}
