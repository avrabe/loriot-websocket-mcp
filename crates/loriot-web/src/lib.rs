//! Leptos web frontend for LoRaWAN sensor monitoring
//!
//! This crate provides the web UI components for the sensor dashboard.

pub mod api;
pub mod app;
pub mod components;
pub mod pages;
pub mod sse;

pub use app::App;
