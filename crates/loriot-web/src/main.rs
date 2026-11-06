//! WASM entry point for the Leptos web frontend
//!
//! This binary is only intended to run as a WASM target.
//! Build with: trunk build (or cargo build --target wasm32-unknown-unknown)

#[cfg(not(target_arch = "wasm32"))]
compile_error!("loriot-web is a WASM frontend — build with `trunk build` or `--target wasm32-unknown-unknown`");

#[cfg(target_arch = "wasm32")]
fn main() {
    use leptos::prelude::*;
    use loriot_web::App;

    console_error_panic_hook::set_once();
    mount_to_body(App);
}
