//! Navigation bar component

use leptos::prelude::*;
use loriot_core::ConnectionStatus;

use crate::sse::subscribe_status;

/// Navigation bar with links and connection status
#[component]
pub fn Navbar() -> impl IntoView {
    let (status_text, set_status_text) = signal("Disconnected".to_string());
    let (status_color, set_status_color) = signal("badge-error".to_string());
    let (status_icon, set_status_icon) = signal("bg-error".to_string());

    // Subscribe to connection status updates
    Effect::new(move |_| {
        leptos::task::spawn_local(async move {
            // The returned handle is dropped right away, as before this change.
            let _ = subscribe_status(move |status| {
                // Update UI based on connection status
                match status {
                    ConnectionStatus::Connected => {
                        set_status_text.set("Connected".to_string());
                        set_status_color.set("badge-success".to_string());
                        set_status_icon.set("bg-success".to_string());
                    }
                    ConnectionStatus::Connecting => {
                        set_status_text.set("Connecting...".to_string());
                        set_status_color.set("badge-warning".to_string());
                        set_status_icon.set("bg-warning".to_string());
                    }
                    ConnectionStatus::Disconnected => {
                        set_status_text.set("Disconnected".to_string());
                        set_status_color.set("badge-error".to_string());
                        set_status_icon.set("bg-error".to_string());
                    }
                    ConnectionStatus::Reconnecting { attempt, max_attempts } => {
                        set_status_text.set(format!("Reconnecting ({}/{})", attempt, max_attempts));
                        set_status_color.set("badge-warning".to_string());
                        set_status_icon.set("bg-warning".to_string());
                    }
                    ConnectionStatus::Error(msg) => {
                        set_status_text.set(format!("Error: {}", msg));
                        set_status_color.set("badge-error".to_string());
                        set_status_icon.set("bg-error".to_string());
                    }
                }
            });
        });
    });

    view! {
        <div class="navbar bg-base-100 shadow-lg">
            <div class="flex-1">
                <a href="/" class="btn btn-ghost text-xl">
                    "LoRaWAN Monitor"
                </a>
            </div>
            <div class="flex-none">
                <ul class="menu menu-horizontal px-1">
                    <li><a href="/">"Dashboard"</a></li>
                    <li><a href="/analytics">"Analytics"</a></li>
                    <li><a href="/weather">"Weather"</a></li>
                    <li><a href="/heatpump">"Heat Pump"</a></li>
                    <li><a href="/thermal">"Thermal"</a></li>
                    <li><a href="/forecast">"Forecast"</a></li>
                    <li><a href="/energy">"Energie"</a></li>
                    <li><a href="/history">"History"</a></li>
                </ul>
                <div class=move || format!("badge gap-2 ml-4 {}", status_color.get())>
                    <div class=move || format!("w-2 h-2 rounded-full animate-pulse {}", status_icon.get())></div>
                    {status_text}
                </div>
            </div>
        </div>
    }
}
