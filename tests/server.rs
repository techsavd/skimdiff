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
