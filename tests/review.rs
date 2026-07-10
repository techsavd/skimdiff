use std::fs;
use std::path::Path;
use std::process::Command;

use axum::body::Body;
use axum::http::{header, Method, Request};
use http_body_util::BodyExt;
use tower::ServiceExt;

use skimdiff::gitx::Repo;
use skimdiff::server::{router, AppState};

fn git_out(dir: &Path, args: &[&str]) -> String {
    let out = Command::new("git")
        .args(args)
        .current_dir(dir)
        .env("GIT_AUTHOR_NAME", "t")
        .env("GIT_AUTHOR_EMAIL", "t@t")
        .env("GIT_COMMITTER_NAME", "t")
        .env("GIT_COMMITTER_EMAIL", "t@t")
        .output()
        .unwrap();
    assert!(out.status.success(), "{}", String::from_utf8_lossy(&out.stderr));
    String::from_utf8_lossy(&out.stdout).into_owned()
}

fn fixture_repo() -> tempfile::TempDir {
    let tmp = tempfile::tempdir().unwrap();
    let d = tmp.path();
    git_out(d, &["init", "-q", "-b", "main"]);
    fs::write(d.join("Main.java"), "class Main {\n    void a() {}\n}\n").unwrap();
    git_out(d, &["add", "."]);
    git_out(d, &["commit", "-q", "-m", "init"]);
    fs::write(d.join("Main.java"), "class Main {\n    void a() { run(); }\n}\n").unwrap();
    tmp
}

async fn req(app: axum::Router, method: Method, uri: &str, body: Option<serde_json::Value>) -> (u16, serde_json::Value) {
    let b = match &body {
        Some(v) => Body::from(v.to_string()),
        None => Body::empty(),
    };
    let mut builder = Request::builder().method(method).uri(uri);
    if body.is_some() {
        builder = builder.header(header::CONTENT_TYPE, "application/json");
    }
    let res = app.oneshot(builder.body(b).unwrap()).await.unwrap();
    let status = res.status().as_u16();
    let bytes = res.into_body().collect().await.unwrap().to_bytes();
    let v = if bytes.is_empty() {
        serde_json::Value::Null
    } else {
        serde_json::from_slice(&bytes).unwrap()
    };
    (status, v)
}

#[tokio::test]
async fn review_state_persists_across_server_instances() {
    let tmp = fixture_repo();

    let repo = Repo::discover(tmp.path()).unwrap();
    let app = router(AppState::new(repo, None));
    let (status, _) = req(
        app.clone(),
        Method::POST,
        "/api/state",
        Some(serde_json::json!({"path": "Main.java", "viewed": true, "note": "check nulls"})),
    )
    .await;
    assert_eq!(status, 200);

    // a fresh router (fresh process, conceptually) sees the same state
    let repo2 = Repo::discover(tmp.path()).unwrap();
    let app2 = router(AppState::new(repo2, None));
    let (_, v) = req(app2, Method::GET, "/api/state", None).await;
    assert_eq!(v["files"]["Main.java"]["viewed"], true);
    assert_eq!(v["files"]["Main.java"]["note"], "check nulls");
}

#[tokio::test]
async fn stage_hunk_moves_change_to_index() {
    let tmp = fixture_repo();
    let repo = Repo::discover(tmp.path()).unwrap();
    let app = router(AppState::new(repo, None));

    let (status, v) = req(
        app,
        Method::POST,
        "/api/hunk",
        Some(serde_json::json!({"path": "Main.java", "hunk": 0, "action": "stage"})),
    )
    .await;
    assert_eq!(status, 200, "{v}");

    let staged = git_out(tmp.path(), &["diff", "--cached", "--name-only"]);
    assert!(staged.contains("Main.java"));
    let unstaged = git_out(tmp.path(), &["diff", "--name-only"]);
    assert!(!unstaged.contains("Main.java"), "no unstaged changes left");
}

#[tokio::test]
async fn discard_hunk_restores_working_tree() {
    let tmp = fixture_repo();
    let repo = Repo::discover(tmp.path()).unwrap();
    let app = router(AppState::new(repo, None));

    let (status, v) = req(
        app,
        Method::POST,
        "/api/hunk",
        Some(serde_json::json!({"path": "Main.java", "hunk": 0, "action": "discard"})),
    )
    .await;
    assert_eq!(status, 200, "{v}");

    let content = fs::read_to_string(tmp.path().join("Main.java")).unwrap();
    assert_eq!(content, "class Main {\n    void a() {}\n}\n");
}

#[tokio::test]
async fn stage_untracked_file_adds_it_and_discard_refuses() {
    let tmp = fixture_repo();
    fs::write(tmp.path().join("New.java"), "class New {}\n").unwrap();

    let repo = Repo::discover(tmp.path()).unwrap();
    let app = router(AppState::new(repo, None));

    let (status, _) = req(
        app.clone(),
        Method::POST,
        "/api/hunk",
        Some(serde_json::json!({"path": "New.java", "hunk": 0, "action": "stage"})),
    )
    .await;
    assert_eq!(status, 200);
    let staged = git_out(tmp.path(), &["diff", "--cached", "--name-only"]);
    assert!(staged.contains("New.java"));

    // discarding an untracked file would delete it; the server must refuse
    fs::write(tmp.path().join("New2.java"), "class New2 {}\n").unwrap();
    let (status, _) = req(
        app,
        Method::POST,
        "/api/hunk",
        Some(serde_json::json!({"path": "New2.java", "hunk": 0, "action": "discard"})),
    )
    .await;
    assert_eq!(status, 400);
    assert!(tmp.path().join("New2.java").exists());
}
