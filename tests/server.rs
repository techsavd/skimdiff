use std::fs;
use std::path::Path;
use std::process::Command;

use axum::body::Body;
use axum::http::Request;
use http_body_util::BodyExt;
use tower::ServiceExt;

use skimdiff::gitx::Repo;
use skimdiff::server::{router, AppState};

fn git(dir: &Path, args: &[&str]) {
    let out = Command::new("git")
        .args(args)
        .current_dir(dir)
        .env("GIT_AUTHOR_NAME", "t")
        .env("GIT_AUTHOR_EMAIL", "t@t")
        .env("GIT_COMMITTER_NAME", "t")
        .env("GIT_COMMITTER_EMAIL", "t@t")
        .output()
        .unwrap();
    assert!(out.status.success());
}

fn fixture_repo() -> tempfile::TempDir {
    let tmp = tempfile::tempdir().unwrap();
    let d = tmp.path();
    git(d, &["init", "-q", "-b", "main"]);
    fs::write(d.join("Main.java"), "class Main {}\n").unwrap();
    git(d, &["add", "."]);
    git(d, &["commit", "-q", "-m", "init"]);
    fs::write(d.join("Main.java"), "class Main { int x; }\n").unwrap();
    tmp
}

async fn get_json(app: axum::Router, uri: &str) -> serde_json::Value {
    let res = app
        .oneshot(Request::builder().uri(uri).body(Body::empty()).unwrap())
        .await
        .unwrap();
    assert_eq!(res.status(), 200, "GET {uri}");
    let bytes = res.into_body().collect().await.unwrap().to_bytes();
    serde_json::from_slice(&bytes).unwrap()
}

#[tokio::test]
async fn api_diff_live_mode_returns_worktree_files() {
    let tmp = fixture_repo();
    let repo = Repo::discover(tmp.path()).unwrap();
    let app = router(AppState::new(repo, None));

    let v = get_json(app, "/api/diff").await;
    assert_eq!(v["mode"], "live");
    let files = v["files"].as_array().unwrap();
    assert_eq!(files.len(), 1);
    assert_eq!(files[0]["path"], "Main.java");
    assert_eq!(files[0]["status"], "modified");
}

#[tokio::test]
async fn api_diff_range_mode_uses_fixed_range() {
    let tmp = fixture_repo();
    let d = tmp.path();
    git(d, &["add", "-A"]);
    git(d, &["commit", "-q", "-m", "second"]);

    let repo = Repo::discover(d).unwrap();
    let app = router(AppState::new(repo, Some("HEAD~1..HEAD".to_string())));

    let v = get_json(app, "/api/diff").await;
    assert_eq!(v["mode"], "range");
    assert_eq!(v["range"], "HEAD~1..HEAD");
    assert_eq!(v["files"].as_array().unwrap().len(), 1);
}

#[tokio::test]
async fn sse_stream_delivers_change_events() {
    use futures::StreamExt;

    let tmp = fixture_repo();
    let repo = Repo::discover(tmp.path()).unwrap();
    let state = AppState::new(repo, None);
    let events = state.events.clone();
    let app = router(state);

    let res = app
        .oneshot(Request::builder().uri("/api/events").body(Body::empty()).unwrap())
        .await
        .unwrap();
    assert_eq!(res.status(), 200);
    let ct = res.headers()["content-type"].to_str().unwrap();
    assert!(ct.contains("text/event-stream"), "got {ct}");

    events.send(()).unwrap();

    let mut stream = res.into_body().into_data_stream();
    let chunk = tokio::time::timeout(std::time::Duration::from_secs(2), stream.next())
        .await
        .expect("event within 2s")
        .expect("stream open")
        .unwrap();
    let text = String::from_utf8_lossy(&chunk);
    assert!(text.contains("change"), "got {text}");
}

#[tokio::test]
async fn usages_endpoint_finds_declarations_and_references() {
    let tmp = fixture_repo();
    fs::write(tmp.path().join("A.java"), "class A {\n    void hit() {}\n}\n").unwrap();
    fs::write(
        tmp.path().join("B.java"),
        "class B {\n    void go(A a) {\n        a.hit();\n    }\n}\n",
    )
    .unwrap();

    let repo = Repo::discover(tmp.path()).unwrap();
    let app = router(AppState::new(repo, None));

    let v = get_json(app, "/api/usages?name=hit").await;
    assert_eq!(v["declarations"].as_array().unwrap().len(), 1);
    assert_eq!(v["declarations"][0]["path"], "A.java");
    assert_eq!(v["references"].as_array().unwrap().len(), 1);
    assert_eq!(v["references"][0]["path"], "B.java");
}

#[tokio::test]
async fn file_endpoint_returns_content_and_rejects_escapes() {
    let tmp = fixture_repo();
    let repo = Repo::discover(tmp.path()).unwrap();
    let app = router(AppState::new(repo, None));

    let v = get_json(app.clone(), "/api/file?path=Main.java").await;
    assert!(v["content"].as_str().unwrap().contains("class Main"));

    let res = app
        .oneshot(
            Request::builder()
                .uri("/api/file?path=../outside.txt")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(res.status(), 400);
}

#[tokio::test]
async fn index_html_is_served() {
    let tmp = fixture_repo();
    let repo = Repo::discover(tmp.path()).unwrap();
    let app = router(AppState::new(repo, None));

    let res = app
        .oneshot(Request::builder().uri("/").body(Body::empty()).unwrap())
        .await
        .unwrap();
    assert_eq!(res.status(), 200);
    let ct = res.headers()["content-type"].to_str().unwrap().to_string();
    assert!(ct.contains("text/html"), "got {ct}");
}
