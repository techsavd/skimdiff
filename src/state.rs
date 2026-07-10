use std::collections::HashMap;
use std::fs;
use std::path::{Path, PathBuf};

use anyhow::Result;
use serde::{Deserialize, Serialize};

#[derive(Debug, Default, Serialize, Deserialize)]
pub struct ReviewState {
    #[serde(default)]
    pub files: HashMap<String, FileReview>,
}

#[derive(Debug, Default, Serialize, Deserialize)]
pub struct FileReview {
    #[serde(default)]
    pub viewed: bool,
    #[serde(default)]
    pub note: String,
}

fn state_file(git_dir: &Path) -> PathBuf {
    git_dir.join("skimdiff").join("state.json")
}

impl ReviewState {
    pub fn load(git_dir: &Path) -> ReviewState {
        fs::read_to_string(state_file(git_dir))
            .ok()
            .and_then(|s| serde_json::from_str(&s).ok())
            .unwrap_or_default()
    }

    pub fn save(&self, git_dir: &Path) -> Result<()> {
        let path = state_file(git_dir);
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent)?;
        }
        fs::write(path, serde_json::to_string_pretty(self)?)?;
        Ok(())
    }

    pub fn update(&mut self, path: &str, viewed: Option<bool>, note: Option<String>) {
        let entry = self.files.entry(path.to_string()).or_default();
        if let Some(v) = viewed {
            entry.viewed = v;
        }
        if let Some(n) = note {
            entry.note = n;
        }
    }
}
