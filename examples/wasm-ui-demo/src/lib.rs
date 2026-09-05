#[macro_use]
extern crate dominator;

mod app;
use app::{App, render};
use wasm_bindgen::prelude::*;

#[wasm_bindgen(start)]
pub fn main() {
    // Initialize panic hook for better error messages
    console_error_panic_hook::set_once();

    // Initialize dwind styles
    dwind::stylesheet();

    // Create app and render
    let app = App::new();
    dominator::append_dom(&dominator::body(), render(app));
}
