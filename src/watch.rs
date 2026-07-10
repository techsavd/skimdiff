use std::path::PathBuf;
use std::time::Duration;

use anyhow::Result;
use ignore::gitignore::{Gitignore, GitignoreBuilder};
use notify::{RecommendedWatcher, RecursiveMode, Watcher};
use tokio::sync::broadcast;

const DEBOUNCE: Duration = Duration::from_millis(300);

/// Watch `root` recursively and send one broadcast event per debounced burst
/// of relevant changes. `.git/` internals and gitignored paths are skipped.
/// The returned watcher must be kept alive.
pub fn start_watcher(root: PathBuf, tx: broadcast::Sender<()>) -> Result<RecommendedWatcher> {
    // fsevents reports canonical paths (/private/var/...) even when watching
    // through a symlink (/var/...), so match against the canonical root
    let canon_root = root.canonicalize().unwrap_or_else(|_| root.clone());
    let matcher = build_ignore(&canon_root);
    let (raw_tx, mut raw_rx) = tokio::sync::mpsc::unbounded_channel::<()>();

    let git_dir = canon_root.join(".git");
    let mut watcher = notify::recommended_watcher(move |res: notify::Result<notify::Event>| {
        let Ok(event) = res else { return };
        let relevant = event.paths.iter().any(|p| {
            if p.starts_with(&git_dir) {
                return false;
            }
            match p.strip_prefix(&canon_root) {
                Ok(rel) => !matcher
                    .matched_path_or_any_parents(rel, p.is_dir())
                    .is_ignore(),
                // outside the tree we can't classify it; err on refreshing
                Err(_) => true,
            }
        });
        if relevant {
            let _ = raw_tx.send(());
        }
    })?;
    watcher.watch(&root, RecursiveMode::Recursive)?;

    // debounce: after the first raw event, swallow everything for DEBOUNCE,
    // then emit a single broadcast event
    tokio::spawn(async move {
        while raw_rx.recv().await.is_some() {
            tokio::time::sleep(DEBOUNCE).await;
            while raw_rx.try_recv().is_ok() {}
            let _ = tx.send(());
        }
    });

    Ok(watcher)
}

fn build_ignore(root: &PathBuf) -> Gitignore {
    let mut b = GitignoreBuilder::new(root);
    b.add(root.join(".gitignore"));
    b.build().unwrap_or_else(|_| Gitignore::empty())
}
