use std::fs;
use std::path::Path;
use std::process::Command;
use std::time::Duration;

use skimdiff::watch::start_watcher;

fn git(dir: &Path, args: &[&str]) {
    let out = Command::new("git").args(args).current_dir(dir).output().unwrap();
    assert!(out.status.success());
}

fn fixture() -> tempfile::TempDir {
    let tmp = tempfile::tempdir().unwrap();
    let d = tmp.path();
    git(d, &["init", "-q", "-b", "main"]);
    fs::write(d.join(".gitignore"), "target/\n").unwrap();
    fs::create_dir(d.join("target")).unwrap();
    tmp
}

#[tokio::test]
async fn file_change_produces_debounced_event() {
    let tmp = fixture();
    let (tx, mut rx) = tokio::sync::broadcast::channel(16);
    let _watcher = start_watcher(tmp.path().to_path_buf(), tx).unwrap();
    // give the watcher a beat to register
    tokio::time::sleep(Duration::from_millis(200)).await;

    fs::write(tmp.path().join("Main.java"), "class Main {}\n").unwrap();

    tokio::time::timeout(Duration::from_secs(3), rx.recv())
        .await
        .expect("expected a change event")
        .unwrap();
}

#[tokio::test]
async fn git_internal_and_ignored_paths_do_not_emit() {
    let tmp = fixture();
    let (tx, mut rx) = tokio::sync::broadcast::channel(16);
    let _watcher = start_watcher(tmp.path().to_path_buf(), tx).unwrap();
    tokio::time::sleep(Duration::from_millis(200)).await;

    fs::write(tmp.path().join(".git").join("somefile"), "x").unwrap();
    fs::write(tmp.path().join("target").join("out.bin"), "x").unwrap();

    let got = tokio::time::timeout(Duration::from_millis(1200), rx.recv()).await;
    assert!(got.is_err(), "ignored paths should not produce events");
}

#[tokio::test]
async fn burst_of_writes_collapses_to_few_events() {
    let tmp = fixture();
    let (tx, mut rx) = tokio::sync::broadcast::channel(64);
    let _watcher = start_watcher(tmp.path().to_path_buf(), tx).unwrap();
    tokio::time::sleep(Duration::from_millis(200)).await;

    for i in 0..20 {
        fs::write(tmp.path().join(format!("f{i}.txt")), "x").unwrap();
    }

    // drain events for a while; a 300ms debounce should collapse 20 writes
    let mut count = 0;
    while tokio::time::timeout(Duration::from_millis(1500), rx.recv())
        .await
        .is_ok()
    {
        count += 1;
    }
    assert!(count >= 1 && count <= 3, "got {count} events, expected 1-3");
}
