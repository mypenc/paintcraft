//! Interface language: English and Indonesian (the device language decides, Settings can override).

use std::sync::atomic::{AtomicU8, Ordering};

/// 0 = follow the device, 1 = English, 2 = Indonesian.
static PREF: AtomicU8 = AtomicU8::new(0);
static DEVICE_ID: AtomicU8 = AtomicU8::new(0);

pub fn init(device_tag: Option<&str>) {
    let id = device_tag.map(|t| t.to_ascii_lowercase()).is_some_and(|t| t.starts_with("id") || t.starts_with("in") || t.starts_with("ms-id"));
    DEVICE_ID.store(u8::from(id), Ordering::Relaxed);
}

pub fn preference() -> &'static str {
    match PREF.load(Ordering::Relaxed) {
        1 => "en",
        2 => "id",
        _ => "auto",
    }
}

pub fn set_preference(code: &str) {
    PREF.store(
        match code {
            "en" => 1,
            "id" => 2,
            _ => 0,
        },
        Ordering::Relaxed,
    );
}

pub fn is_indonesian() -> bool {
    match PREF.load(Ordering::Relaxed) {
        1 => false,
        2 => true,
        _ => DEVICE_ID.load(Ordering::Relaxed) == 1,
    }
}

/// Translate an interface string (English is the key).
pub fn t(s: &str) -> String {
    if is_indonesian() {
        if let Some(x) = indonesian(s) {
            return x.to_string();
        }
    }
    s.to_string()
}

fn indonesian(s: &str) -> Option<&'static str> {
    Some(match s {
        "Cancel" => "Batal",
        "Close" => "Tutup",
        "OK" => "OK",
        "Back" => "Kembali",
        "Edit" => "Sunting",
        "Select" => "Pilih",
        "Done" => "Selesai",
        "Empty" => "Kosongkan",
        "Create" => "Buat",
        "Delete" => "Hapus",
        "Duplicate" => "Duplikat",
        "Rename" => "Ganti Nama",
        "Add" => "Tambah",
        "Apply" => "Terapkan",
        "Save" => "Simpan",
        "Saved" => "Tersimpan",
        "Share" => "Bagikan",
        "Import" => "Impor",
        "Folder" => "Folder",
        "Title" => "Judul",
        "Time" => "Waktu",
        "Artist" => "Artis",
        "Description" => "Deskripsi",
        "left" => "tersisa",
        "New" => "Baru",
        "New Folder" => "Folder Baru",
        "Import Image" => "Impor Gambar",
        "Illustration" => "Ilustrasi",
        "Custom" => "Kustom",
        "For Printing" => "Untuk Dicetak",
        "HD (Quality)" => "HD (Kualitas)",
        "Header X" => "Tajuk X",
        "Chat Trace" => "Jejak Obrolan",
        "Vertical" => "Vertikal",
        "Postcard (JP)" => "Kartu pos (JP)",
        "My Gallery" => "Galeri Saya",
        "Recently Deleted" => "Karya yang Baru Dihapus",
        "Delete Permanently" => "Hapus Permanen",
        "Save Image (PNG / JPEG / PSD)" => "Simpan Gambar (PNG / JPEG / PSD)",
        "Show Storage Path" => "Tampilkan Lokasi Penyimpanan",
        "Please choose" => "Silakan pilih",
        "Untitled" => "Tak berjudul",
        "That picture is too large to paint on a phone." => "Gambar itu terlalu besar untuk dilukis di ponsel.",
        "(copy)" => "(salinan)",
        "Select an artwork first." => "Pilih karya terlebih dahulu.",
        "Tap + to start a new canvas." => "Ketuk + untuk membuat kanvas baru.",
        "Enter the width and height in pixels." => "Masukkan lebar dan tinggi dalam piksel.",
        "The canvas is too large. At most" => "Kanvas terlalu besar. Maksimal",
        "This cannot be undone." => "Tindakan ini tidak dapat dibatalkan.",
        "Move to Recently Deleted?" => "Pindahkan ke Karya yang Baru Dihapus?",
        "Delete everything in Recently Deleted for good?" => "Hapus semua karya di Baru Dihapus secara permanen?",
        "Nothing here." => "Tidak ada apa-apa di sini.",
        "Move to Folder" => "Pindahkan ke Folder",
        "Storage" => "Penyimpanan",
        "Artworks are kept in PaintCraft's private storage on this device and saved automatically." => "Karya disimpan di penyimpanan pribadi PaintCraft pada perangkat ini dan tersimpan otomatis.",
        "Browser preview: artworks live in memory and are lost when you close the page." => "Pratinjau browser: karya hanya ada di memori dan hilang saat halaman ditutup.",
        "Browser preview: the file is downloaded." => "Pratinjau browser: berkas akan diunduh.",
        "Artwork Information" => "Informasi Karya",
        "Canvas Size" => "Ukuran Kanvas",
        "File Size" => "Ukuran File",
        "Type" => "Jenis Karya",
        "Created" => "Tanggal Kreasi",
        "Updated" => "Tanggal Pembaruan",
        "Save Image" => "Simpan Gambar",
        "How to Use" => "Cara Menggunakan",
        "Settings" => "Pengaturan",
        "Back to My Gallery" => "Kembali ke Galeri Saya",
        "Draw with one finger." => "Gambar dengan satu jari.",
        "Pinch with two fingers to zoom, drag with two to move the canvas." => "Cubit dengan dua jari untuk zoom, geser dengan dua jari untuk memindahkan kanvas.",
        "The brush icon opens the tool menu; tap Brush there for the brush list." => "Ikon kuas membuka menu alat; ketuk Kuas untuk daftar kuas.",
        "The square swatch opens the colour panel." => "Kotak warna membuka panel warna.",
        "The stack icon opens the layers." => "Ikon tumpukan membuka lapisan.",
        "Your work is saved automatically." => "Karya Anda tersimpan otomatis.",
        "Transform" => "Mengubah",
        "Magic Wand" => "Tongkat Sihir",
        "Lasso" => "Laso",
        "Filter" => "Filter",
        "Brush" => "Kuas",
        "Eraser" => "Penghapus",
        "Smudge" => "Noda",
        "Blur" => "Kekaburan",
        "Selection" => "Seleksi",
        "Fill" => "Ember Cat",
        "Shapes" => "Bentuk",
        "Text" => "Teks",
        "Eyedropper" => "Pipet",
        "Canvas" => "Kanvas",
        "Deselect" => "Batal Pilih",
        "Invert" => "Balik",
        "Clear" => "Hapus Isi",
        "Ruler" => "Penggaris",
        "Symmetry Ruler" => "Penggaris Simetri",
        "Off" => "Mati",
        "Straight" => "Lurus",
        "Left-Right" => "Kiri-Kanan",
        "Up-Down" => "Atas-Bawah",
        "4-Way" => "4 Arah",
        "Touch" => "Sentuhan",
        "Finger draws" => "Jari menggambar",
        "Finger pans" => "Jari menggeser",
        "Two fingers always pan and zoom." => "Dua jari selalu menggeser dan zoom.",
        "Stroke stabilization" => "Stabilisasi goresan",
        "Language" => "Bahasa",
        "Line" => "Garis",
        "Rectangle" => "Persegi",
        "Ellipse" => "Elips",
        "Flip canvas ↔" => "Balik kanvas ↔",
        "Flip canvas ↕" => "Balik kanvas ↕",
        "Flip layer ↔" => "Balik lapisan ↔",
        "Flip layer ↕" => "Balik lapisan ↕",
        "Fit to screen" => "Sesuaikan layar",
        "Blend mode" => "Mode campuran",
        "Layer name" => "Nama lapisan",
        "Brushes" => "Kuas",
        "All" => "Semua",
        "Pens" => "Pena",
        "Sketch" => "Sketsa",
        "Ink" => "Tinta",
        "Airbrush" => "Kuas Semprot",
        "Paint" => "Cat",
        "Marker" => "Spidol",
        "Hardness" => "Kekerasan",
        "Flow" => "Aliran",
        "Spacing" => "Jarak",
        "Color" => "Warna",
        "Palette" => "Palet",
        "Recent" => "Terbaru",
        "Layers" => "Lapisan",
        "Merge down" => "Gabung ke bawah",
        "Flip" => "Balik",
        "Clip" => "Klip",
        "Lock α" => "Kunci α",
        "Background" => "Latar Belakang",
        "Gaussian Blur" => "Gaussian Blur",
        "Sharpen" => "Pertajam",
        "Add Noise" => "Tambah Derau",
        "Brightness / Contrast" => "Kecerahan / Kontras",
        "Invert Colors" => "Balik Warna",
        "Black & White" => "Hitam Putih",
        "Radius" => "Radius",
        "Amount" => "Jumlah",
        "Brightness" => "Kecerahan",
        "Contrast" => "Kontras",
        "Dip Pen (Smooth)" => "Dip Pen (Halus)",
        "Dip Pen (Hard)" => "Dip Pen (Kasar)",
        "Felt Tip Pen" => "Pena Felt Tip",
        "Digital Pen" => "Pena Digital",
        "Soft Pen" => "Pena Lembut",
        "Pencil" => "Pensil",
        "Soft Pencil" => "Pensil Lembut",
        "Charcoal" => "Arang",
        "Airbrush (Soft)" => "Kuas Semprot (Lembut)",
        "Airbrush (Medium)" => "Kuas Semprot (Sedang)",
        "Watercolor" => "Cat Air",
        "Flat Brush" => "Kuas Datar",
        "Highlighter" => "Stabilo",
        _ => return None,
    })
}
