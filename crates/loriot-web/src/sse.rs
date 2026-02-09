//! Server-Sent Events (SSE) subscription for real-time updates

use loriot_core::{ConnectionStatus, SensorReading};

/// SSE endpoint URLs
pub const SSE_READINGS: &str = "/sse/readings";
pub const SSE_STATUS: &str = "/sse/status";

/// Create an EventSource subscription for sensor readings
#[cfg(feature = "hydrate")]
pub fn subscribe_readings<F>(on_reading: F) -> Result<web_sys::EventSource, String>
where
    F: Fn(SensorReading) + 'static,
{
    use wasm_bindgen::prelude::*;
    use wasm_bindgen::JsCast;

    let es = web_sys::EventSource::new(SSE_READINGS).map_err(|e| format!("{:?}", e))?;

    let callback = Closure::wrap(Box::new(move |event: web_sys::MessageEvent| {
        if let Some(data) = event.data().as_string() {
            if let Ok(reading) = serde_json::from_str::<SensorReading>(&data) {
                on_reading(reading);
            }
        }
    }) as Box<dyn FnMut(_)>);

    es.add_event_listener_with_callback("reading", callback.as_ref().unchecked_ref())
        .map_err(|e| format!("{:?}", e))?;

    callback.forget(); // Keep the closure alive

    Ok(es)
}

/// Create an EventSource subscription for connection status
#[cfg(feature = "hydrate")]
pub fn subscribe_status<F>(on_status: F) -> Result<web_sys::EventSource, String>
where
    F: Fn(ConnectionStatus) + 'static,
{
    use wasm_bindgen::prelude::*;
    use wasm_bindgen::JsCast;

    let es = web_sys::EventSource::new(SSE_STATUS).map_err(|e| format!("{:?}", e))?;

    let callback = Closure::wrap(Box::new(move |event: web_sys::MessageEvent| {
        if let Some(data) = event.data().as_string() {
            if let Ok(status) = serde_json::from_str::<ConnectionStatus>(&data) {
                on_status(status);
            }
        }
    }) as Box<dyn FnMut(_)>);

    es.add_event_listener_with_callback("status", callback.as_ref().unchecked_ref())
        .map_err(|e| format!("{:?}", e))?;

    callback.forget();

    Ok(es)
}

// Stub for server-side rendering
#[cfg(not(feature = "hydrate"))]
pub fn subscribe_readings<F>(_on_reading: F) -> Result<(), String>
where
    F: Fn(SensorReading) + 'static,
{
    Ok(())
}

#[cfg(not(feature = "hydrate"))]
pub fn subscribe_status<F>(_on_status: F) -> Result<(), String>
where
    F: Fn(ConnectionStatus) + 'static,
{
    Ok(())
}
