//! WASM entry point for the Leptos web frontend
//!
//! This binary is only intended to run as a WASM target.
//! Build with: trunk build (or cargo build --target wasm32-unknown-unknown)

/// Native stub so `cargo build/clippy/test --workspace` works on the host.
/// The real frontend is the wasm32 build below.
#[cfg(not(target_arch = "wasm32"))]
fn main() {
    eprintln!("loriot-web is a WASM frontend: build it with `trunk build`");
    std::process::exit(2);
}

#[cfg(target_arch = "wasm32")]
fn main() {
    use leptos::prelude::*;
    use loriot_web::App;

    console_error_panic_hook::set_once();
    mount_to_body(App);
}
