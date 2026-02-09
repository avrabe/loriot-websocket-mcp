//! Server-Sent Events (SSE) handlers for real-time updates

use axum::{
    extract::State,
    response::sse::{Event, Sse},
};
use futures::stream::Stream;
use std::convert::Infallible;
use tokio_stream::wrappers::BroadcastStream;
use tokio_stream::StreamExt;

use crate::AppState;

/// GET /sse/readings - Stream new sensor readings as they arrive
pub async fn readings_stream(
    State(state): State<AppState>,
) -> Sse<impl Stream<Item = Result<Event, Infallible>>> {
    let rx = state.reading_tx.subscribe();
    let stream = BroadcastStream::new(rx).filter_map(|result| {
        result.ok().map(|reading| {
            let data = serde_json::to_string(&reading).unwrap_or_default();
            Ok(Event::default().event("reading").data(data))
        })
    });

    Sse::new(stream)
}

/// GET /sse/status - Stream connection status updates
pub async fn status_stream(
    State(state): State<AppState>,
) -> Sse<impl Stream<Item = Result<Event, Infallible>>> {
    // Send current status immediately
    let current_status = state.connection_status.read().await.clone();

    let rx = state.status_tx.subscribe();
    let status_stream = BroadcastStream::new(rx).filter_map(|result| {
        result.ok().map(|status| {
            let data = serde_json::to_string(&status).unwrap_or_default();
            Ok(Event::default().event("status").data(data))
        })
    });

    // Prepend current status to the stream
    let initial = futures::stream::once(async move {
        let data = serde_json::to_string(&current_status).unwrap_or_default();
        Ok(Event::default().event("status").data(data))
    });

    Sse::new(initial.chain(status_stream))
}
