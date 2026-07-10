use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;

use anyhow::{bail, Context, Result};

use crate::diff::{parse_patch, synthetic_added, FileDiff};

pub struct Repo {
    pub root: PathBuf,
}

impl Repo {
    /// Find the repo containing `dir`, erroring outside any git repo.
    pub fn discover(dir: &Path) -> Result<Repo> {
        let out = Command::new("git")
            .args(["rev-parse", "--show-toplevel"])
            .current_dir(dir)
            .output()
            .context("running git")?;
        if !out.status.success() {
            bail!("not a git repository: {}", dir.display());
        }
        let root = PathBuf::from(String::from_utf8(out.stdout)?.trim());
        Ok(Repo { root })
    }

    pub fn git(&self, args: &[&str]) -> Result<String> {
        let out = Command::new("git")
            .args(args)
            .current_dir(&self.root)
            .output()
            .context("running git")?;
        if !out.status.success() {
            bail!(
                "git {} failed: {}",
                args.join(" "),
                String::from_utf8_lossy(&out.stderr)
            );
        }
        Ok(String::from_utf8_lossy(&out.stdout).into_owned())
    }

    /// Staged + unstaged changes vs HEAD, plus untracked files as all-added.
    pub fn worktree_diff(&self) -> Result<Vec<FileDiff>> {
        let patch = self.git(&["diff", "HEAD", "--patch", "--no-color", "--find-renames"])?;
        let mut files = parse_patch(&patch);
        for path in self.untracked()? {
            let full = self.root.join(&path);
            match fs::read(&full) {
                Ok(bytes) => match String::from_utf8(bytes) {
                    Ok(text) => files.push(synthetic_added(&path, &text)),
                    Err(_) => {
                        let mut f = synthetic_added(&path, "");
                        f.is_binary = true;
                        files.push(f);
                    }
                },
                Err(_) => continue, // vanished between listing and read
            }
        }
        Ok(files)
    }

    /// Diff for an arbitrary range/commit, e.g. "main..feat", "HEAD~3", a sha.
    pub fn range_diff(&self, range: &str) -> Result<Vec<FileDiff>> {
        let patch = self.git(&["diff", "--patch", "--no-color", "--find-renames", range])?;
        Ok(parse_patch(&patch))
    }

    /// Absolute path of the .git directory (handles worktrees).
    pub fn git_dir(&self) -> Result<std::path::PathBuf> {
        Ok(std::path::PathBuf::from(
            self.git(&["rev-parse", "--absolute-git-dir"])?.trim(),
        ))
    }

    pub fn add(&self, path: &str) -> Result<()> {
        self.git(&["add", "--", path]).map(|_| ())
    }

    pub fn is_tracked(&self, path: &str) -> bool {
        self.git(&["ls-files", "--error-unmatch", "--", path]).is_ok()
    }

    /// Apply a patch via stdin. `cached` targets the index (stage),
    /// `reverse` reverse-applies to the working tree (discard).
    pub fn apply_patch(&self, patch: &str, cached: bool, reverse: bool) -> Result<()> {
        use std::io::Write;
        use std::process::Stdio;
        let mut args = vec!["apply"];
        if cached {
            args.push("--cached");
        }
        if reverse {
            args.push("--reverse");
        }
        args.push("-");
        let mut child = Command::new("git")
            .args(&args)
            .current_dir(&self.root)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .context("spawning git apply")?;
        child
            .stdin
            .as_mut()
            .expect("piped stdin")
            .write_all(patch.as_bytes())?;
        let out = child.wait_with_output()?;
        if !out.status.success() {
            bail!("git apply failed: {}", String::from_utf8_lossy(&out.stderr));
        }
        Ok(())
    }

    /// Tracked plus untracked (non-ignored) files, repo-relative.
    pub fn all_files(&self) -> Result<Vec<String>> {
        let tracked = self.git(&["ls-files"])?;
        let mut files: Vec<String> = tracked.lines().map(|s| s.to_string()).collect();
        files.extend(self.untracked()?);
        Ok(files)
    }

    fn untracked(&self) -> Result<Vec<String>> {
        let out = self.git(&["ls-files", "--others", "--exclude-standard"])?;
        Ok(out.lines().map(|s| s.to_string()).collect())
    }
}
