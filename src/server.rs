use std::sync::Arc;

use axum::extract::State;
use axum::http::{header, StatusCode, Uri};
use axum::response::{IntoResponse, Response};
use axum::response::sse::{Event, KeepAlive, Sse};
use axum::routing::get;
use axum::{Json, Router};
use futures::StreamExt;
use rust_embed::RustEmbed;
use serde_json::json;
use tokio::sync::broadcast;
use tokio_stream::wrappers::BroadcastStream;

use crate::gitx::Repo;

#[derive(RustEmbed)]
#[folder = "web/dist/"]
struct Assets;

pub struct AppState {
    pub repo: Repo,
    pub range: Option<String>,
    pub events: broadcast::Sender<()>,
}

impl AppState {
    pub fn new(repo: Repo, range: Option<String>) -> Arc<AppState> {
        let (events, _) = broadcast::channel(64);
        Arc::new(AppState { repo, range, events })
    }
}

pub fn router(state: Arc<AppState>) -> Router {
    Router::new()
        .route("/api/diff", get(api_diff))
        .route("/api/events", get(api_events))
        .fallback(static_asset)
        .with_state(state)
}

async fn api_events(
    State(state): State<Arc<AppState>>,
) -> Sse<impl futures::Stream<Item = Result<Event, std::convert::Infallible>>> {
    let rx = state.events.subscribe();
    let stream = BroadcastStream::new(rx)
        .filter_map(|msg| futures::future::ready(msg.ok().map(|_| Ok(Event::default().data("change")))));
    Sse::new(stream).keep_alive(KeepAlive::default())
}

async fn api_diff(State(state): State<Arc<AppState>>) -> Response {
    let result = match &state.range {
        Some(r) => state.repo.range_diff(r).map(|files| {
            json!({ "mode": "range", "range": r, "files": files })
        }),
        None => state
            .repo
            .worktree_diff()
            .map(|files| json!({ "mode": "live", "files": files })),
    };
    match result {
        Ok(v) => Json(v).into_response(),
        Err(e) => (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(json!({ "error": e.to_string() })),
        )
            .into_response(),
    }
}

async fn static_asset(uri: Uri) -> Response {
    let path = uri.path().trim_start_matches('/');
    let path = if path.is_empty() { "index.html" } else { path };
    match Assets::get(path) {
        Some(f) => {
            let mime = mime_guess::from_path(path).first_or_octet_stream();
            ([(header::CONTENT_TYPE, mime.as_ref().to_string())], f.data).into_response()
        }
        // SPA fallback
        None => match Assets::get("index.html") {
            Some(f) => ([(header::CONTENT_TYPE, "text/html".to_string())], f.data).into_response(),
            None => (StatusCode::NOT_FOUND, "ui not built").into_response(),
        },
    }
}
