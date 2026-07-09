use std::fs;
use std::path::Path;
use std::process::Command;

use skimdiff::diff::{parse_patch, FileStatus, LineKind};
use skimdiff::gitx::Repo;

fn git(dir: &Path, args: &[&str]) {
    let out = Command::new("git")
        .args(args)
        .current_dir(dir)
        .env("GIT_AUTHOR_NAME", "t")
        .env("GIT_AUTHOR_EMAIL", "t@t")
        .env("GIT_COMMITTER_NAME", "t")
        .env("GIT_COMMITTER_EMAIL", "t@t")
        .output()
        .expect("git runs");
    assert!(
        out.status.success(),
        "git {:?} failed: {}",
        args,
        String::from_utf8_lossy(&out.stderr)
    );
}

/// Repo with one committed file, then a modification, a new untracked file,
/// and a deleted file.
fn fixture_repo() -> tempfile::TempDir {
    let tmp = tempfile::tempdir().unwrap();
    let d = tmp.path();
    git(d, &["init", "-q", "-b", "main"]);
    fs::write(d.join("Main.java"), "class Main {\n    void a() {}\n    void b() {}\n}\n").unwrap();
    fs::write(d.join("Gone.java"), "class Gone {}\n").unwrap();
    git(d, &["add", "."]);
    git(d, &["commit", "-q", "-m", "init"]);
    // modify
    fs::write(d.join("Main.java"), "class Main {\n    void a() { run(); }\n    void b() {}\n}\n").unwrap();
    // delete
    fs::remove_file(d.join("Gone.java")).unwrap();
    // untracked
    fs::write(d.join("New.java"), "class New {\n}\n").unwrap();
    tmp
}

#[test]
fn parse_patch_extracts_files_hunks_and_line_numbers() {
    let patch = "\
diff --git a/Main.java b/Main.java
index 1111111..2222222 100644
--- a/Main.java
+++ b/Main.java
@@ -1,4 +1,4 @@
 class Main {
-    void a() {}
+    void a() { run(); }
     void b() {}
 }
";
    let files = parse_patch(patch);
    assert_eq!(files.len(), 1);
    let f = &files[0];
    assert_eq!(f.path, "Main.java");
    assert_eq!(f.status, FileStatus::Modified);
    assert_eq!(f.hunks.len(), 1);
    let h = &f.hunks[0];
    assert_eq!((h.old_start, h.new_start), (1, 1));
    let kinds: Vec<LineKind> = h.lines.iter().map(|l| l.kind).collect();
    assert_eq!(
        kinds,
        vec![
            LineKind::Context,
            LineKind::Del,
            LineKind::Add,
            LineKind::Context,
            LineKind::Context
        ]
    );
    // line numbers: deleted line has old number only, added has new only
    let del = &h.lines[1];
    assert_eq!((del.old_no, del.new_no), (Some(2), None));
    let add = &h.lines[2];
    assert_eq!((add.old_no, add.new_no), (None, Some(2)));
    let last = &h.lines[4];
    assert_eq!((last.old_no, last.new_no), (Some(4), Some(4)));
}

#[test]
fn parse_patch_handles_added_deleted_and_renamed_files() {
    let patch = "\
diff --git a/Old.java b/Old.java
deleted file mode 100644
index 1111111..0000000
--- a/Old.java
+++ /dev/null
@@ -1,1 +0,0 @@
-class Old {}
diff --git a/Fresh.java b/Fresh.java
new file mode 100644
index 0000000..2222222
--- /dev/null
+++ b/Fresh.java
@@ -0,0 +1,1 @@
+class Fresh {}
diff --git a/A.java b/B.java
similarity index 100%
rename from A.java
rename to B.java
";
    let files = parse_patch(patch);
    assert_eq!(files.len(), 3);
    assert_eq!(files[0].status, FileStatus::Deleted);
    assert_eq!(files[0].path, "Old.java");
    assert_eq!(files[1].status, FileStatus::Added);
    assert_eq!(files[1].path, "Fresh.java");
    assert_eq!(files[2].status, FileStatus::Renamed);
    assert_eq!(files[2].path, "B.java");
    assert_eq!(files[2].old_path.as_deref(), Some("A.java"));
    assert!(files[2].hunks.is_empty());
}

#[test]
fn parse_patch_flags_binary_files() {
    let patch = "\
diff --git a/img.png b/img.png
index 1111111..2222222 100644
Binary files a/img.png and b/img.png differ
";
    let files = parse_patch(patch);
    assert_eq!(files.len(), 1);
    assert!(files[0].is_binary);
    assert!(files[0].hunks.is_empty());
}

#[test]
fn worktree_diff_includes_modified_deleted_and_untracked() {
    let tmp = fixture_repo();
    let repo = Repo::discover(tmp.path()).unwrap();
    let mut files = repo.worktree_diff().unwrap();
    files.sort_by(|a, b| a.path.cmp(&b.path));

    let paths: Vec<&str> = files.iter().map(|f| f.path.as_str()).collect();
    assert_eq!(paths, vec!["Gone.java", "Main.java", "New.java"]);

    let gone = &files[0];
    assert_eq!(gone.status, FileStatus::Deleted);

    let main = &files[1];
    assert_eq!(main.status, FileStatus::Modified);
    assert_eq!(main.hunks.len(), 1);

    // untracked file appears as all-added with content
    let new = &files[2];
    assert_eq!(new.status, FileStatus::Added);
    assert_eq!(new.hunks.len(), 1);
    let added: Vec<&str> = new.hunks[0]
        .lines
        .iter()
        .filter(|l| l.kind == LineKind::Add)
        .map(|l| l.content.as_str())
        .collect();
    assert_eq!(added, vec!["class New {", "}"]);
}

#[test]
fn range_diff_between_commits() {
    let tmp = fixture_repo();
    let d = tmp.path();
    git(d, &["add", "-A"]);
    git(d, &["commit", "-q", "-m", "second"]);

    let repo = Repo::discover(d).unwrap();
    let files = repo.range_diff("HEAD~1..HEAD").unwrap();
    let mut paths: Vec<&str> = files.iter().map(|f| f.path.as_str()).collect();
    paths.sort();
    assert_eq!(paths, vec!["Gone.java", "Main.java", "New.java"]);
}

#[test]
fn discover_fails_outside_a_repo() {
    let tmp = tempfile::tempdir().unwrap();
    assert!(Repo::discover(tmp.path()).is_err());
}

#[test]
fn paired_del_add_lines_get_word_level_spans() {
    let patch = "\
diff --git a/Main.java b/Main.java
index 1111111..2222222 100644
--- a/Main.java
+++ b/Main.java
@@ -1,4 +1,4 @@
 class Main {
-    void a() {}
+    void a() { run(); }
     void b() {}
 }
";
    let files = parse_patch(patch);
    let h = &files[0].hunks[0];
    let del = &h.lines[1];
    let add = &h.lines[2];
    // both sides of the pair carry spans; context lines carry none
    let del_spans = del.spans.as_ref().expect("del spans");
    let add_spans = add.spans.as_ref().expect("add spans");
    assert!(h.lines[0].spans.is_none());
    // reassembling spans reproduces the line
    let joined: String = add_spans.iter().map(|s| s.text.as_str()).collect();
    assert_eq!(joined, add.content);
    let joined_del: String = del_spans.iter().map(|s| s.text.as_str()).collect();
    assert_eq!(joined_del, del.content);
    // the unchanged prefix is marked unchanged, and something is marked changed
    assert!(add_spans.iter().any(|s| s.changed));
    assert!(add_spans.first().map(|s| !s.changed).unwrap_or(false));
}
