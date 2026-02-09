//! WASM entry point for the Leptos web frontend

use leptos::prelude::*;
use loriot_web::App;

fn main() {
    // Initialize logging for WASM
    console_error_panic_hook::set_once();

    // Mount the app to the DOM
    mount_to_body(App);
}
