//! Discover local GitHub Actions workflow YAML files from a file or directory path.

use std::fs;
use std::path::{Path, PathBuf};
use thiserror::Error;

#[derive(Debug, Error)]
pub enum DiscoverError {
    #[error("path not found: {0}")]
    NotFound(PathBuf),
    #[error("not a workflow file or directory: {0}")]
    NotAWorkflowPath(PathBuf),
    #[error(transparent)]
    Io(#[from] std::io::Error),
}

pub fn discover_workflows(path: &Path) -> Result<Vec<PathBuf>, DiscoverError> {
    if !path.exists() {
        return Err(DiscoverError::NotFound(path.to_path_buf()));
    }
    if path.is_file() {
        return if is_yaml(path) {
            Ok(vec![path.to_path_buf()])
        } else {
            Err(DiscoverError::NotAWorkflowPath(path.to_path_buf()))
        };
    }
    let search = {
        let nested = path.join(".github/workflows");
        if nested.is_dir() {
            nested
        } else {
            path.to_path_buf()
        }
    };
    let mut out = Vec::new();
    for entry in fs::read_dir(&search)? {
        let entry = entry?;
        let p = entry.path();
        if p.is_file() && is_yaml(&p) {
            out.push(p);
        }
    }
    out.sort();
    Ok(out)
}

fn is_yaml(path: &Path) -> bool {
    matches!(
        path.extension().and_then(|s| s.to_str()),
        Some("yml") | Some("yaml")
    )
}
