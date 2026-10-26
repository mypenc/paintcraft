//! PaintCraft in the browser: the phone interface of the Android app (`paintcraft-ui`) running
//! on PhotoCraft's engine compiled to WebAssembly, so it can be tried from any phone's browser.
//! Artworks live in memory; saving downloads a file.

#[cfg(target_arch = "wasm32")]
mod web;

fn main() {
    #[cfg(target_arch = "wasm32")]
    web::start();
}
