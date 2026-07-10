use std::sync::Arc;

use axum::extract::State;
use axum::http::{header, StatusCode, Uri};
use axum::response::{IntoResponse, Response};
use axum::response::sse::{Event, KeepAlive, Sse};
use axum::routing::{get, post};
use axum::{Json, Router};
use futures::StreamExt;
use rust_embed::RustEmbed;
use serde_json::json;
use tokio::sync::broadcast;
use tokio_stream::wrappers::BroadcastStream;

use crate::gitx::Repo;
use crate::state::ReviewState;
use serde::Deserialize;

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
        .route("/api/state", get(api_state_get).post(api_state_post))
        .route("/api/hunk", post(api_hunk))
        .fallback(static_asset)
        .with_state(state)
}

fn err(status: StatusCode, e: impl ToString) -> Response {
    (status, Json(json!({ "error": e.to_string() }))).into_response()
}

async fn api_state_get(State(state): State<Arc<AppState>>) -> Response {
    match state.repo.git_dir() {
        Ok(gd) => Json(ReviewState::load(&gd)).into_response(),
        Err(e) => err(StatusCode::INTERNAL_SERVER_ERROR, e),
    }
}

#[derive(Deserialize)]
struct StateUpdate {
    path: String,
    viewed: Option<bool>,
    note: Option<String>,
}

async fn api_state_post(
    State(state): State<Arc<AppState>>,
    Json(u): Json<StateUpdate>,
) -> Response {
    let gd = match state.repo.git_dir() {
        Ok(gd) => gd,
        Err(e) => return err(StatusCode::INTERNAL_SERVER_ERROR, e),
    };
    let mut rs = ReviewState::load(&gd);
    rs.update(&u.path, u.viewed, u.note);
    match rs.save(&gd) {
        Ok(()) => Json(rs).into_response(),
        Err(e) => err(StatusCode::INTERNAL_SERVER_ERROR, e),
    }
}

#[derive(Deserialize)]
struct HunkAction {
    path: String,
    hunk: usize,
    action: String,
}

async fn api_hunk(State(state): State<Arc<AppState>>, Json(a): Json<HunkAction>) -> Response {
    if state.range.is_some() {
        return err(StatusCode::BAD_REQUEST, "hunk actions only apply in live mode");
    }
    let stage = match a.action.as_str() {
        "stage" => true,
        "discard" => false,
        other => return err(StatusCode::BAD_REQUEST, format!("unknown action {other}")),
    };
    let files = match state.repo.worktree_diff() {
        Ok(f) => f,
        Err(e) => return err(StatusCode::INTERNAL_SERVER_ERROR, e),
    };
    let Some(file) = files.iter().find(|f| f.path == a.path) else {
        return err(StatusCode::NOT_FOUND, format!("no diff for {}", a.path));
    };

    let untracked = file.status == crate::diff::FileStatus::Added && !state.repo.is_tracked(&a.path);
    let result = if untracked {
        if stage {
            state.repo.add(&a.path)
        } else {
            return err(
                StatusCode::BAD_REQUEST,
                "refusing to discard an untracked file (it would be deleted)",
            );
        }
    } else {
        match crate::diff::hunk_patch(file, a.hunk) {
            Some(patch) => state.repo.apply_patch(&patch, stage, !stage),
            None => return err(StatusCode::NOT_FOUND, format!("no hunk {} in {}", a.hunk, a.path)),
        }
    };
    match result {
        Ok(()) => {
            // nudge connected browsers to refresh
            let _ = state.events.send(());
            Json(json!({ "ok": true })).into_response()
        }
        Err(e) => err(StatusCode::INTERNAL_SERVER_ERROR, e),
    }
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
