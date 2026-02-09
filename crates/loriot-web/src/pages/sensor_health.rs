use leptos::prelude::*;
use crate::components::Navbar;

#[component]
pub fn SensorHealthPage() -> impl IntoView {
    view! {
        <Navbar />
        <div class="container mx-auto p-4">
            <h1 class="text-2xl font-bold mb-4">"Sensor Health"</h1>
            <p>"Sensor health monitoring coming soon."</p>
        </div>
    }
}
