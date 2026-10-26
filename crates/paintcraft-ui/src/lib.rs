//! PaintCraft's touch interface: an ibisPaint-style gallery and canvas on top of PhotoCraft's
//! engine. Shared by the Android app (`paintcraft-android`) and the browser preview
//! (`paintcraft-web`); each supplies a [`platform::Platform`].

pub mod app;
pub mod brushes;
pub mod editor;
pub mod exporter;
pub mod gallery;
pub mod i18n;
pub mod icons;
pub mod platform;
pub mod store;
pub mod ui;

pub use app::PaintApp;
